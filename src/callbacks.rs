//! MIR analysis callbacks for rustc plugin system.
//! Detects use-after-drop, double-drop, and ownership violations.
extern crate rustc_driver;
extern crate rustc_hir;

use std::path::PathBuf;

use log::debug;
use rustc_driver::Compilation;
use rustc_hir::def_id::LOCAL_CRATE;
use rustc_interface::interface;
use rustc_middle::mir::Body;
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::ty::{Instance, TyCtxt, TypingEnv};

pub struct TaintAnaCallbacks {
    file_name: String,
    output_directory: PathBuf,
}

impl TaintAnaCallbacks {
    pub fn new() -> Self {
        Self {
            file_name: String::new(),
            output_directory: PathBuf::default(),
        }
    }
}

impl rustc_driver::Callbacks for TaintAnaCallbacks {
    fn config(&mut self, config: &mut rustc_interface::interface::Config) {
        self.file_name = config
            .input
            .source_name()
            .prefer_remapped_unconditionally()
            .to_string();
        debug!("Processing input file: {}", self.file_name);
        match &config.output_dir {
            None => {
                self.output_directory = std::env::temp_dir();
                self.output_directory.pop();
            }
            Some(path_buf) => self.output_directory.push(path_buf.as_path()),
        }
    }
    
    fn after_analysis(
        &mut self,
        compiler: &rustc_interface::interface::Compiler,
        tcx: TyCtxt<'_>,
    ) -> rustc_driver::Compilation {
        compiler.sess.dcx().abort_if_errors();
        if self
            .output_directory
            .to_str()
            .expect("valid string")
            .contains("/build/")
        {
            // No need to analyze a build script, but do generate code.
            return Compilation::Continue;
        }
        self.analyze_crate(compiler, tcx);
        // Continue compilation to allow cargo to work properly
        Compilation::Continue
    }
}

impl TaintAnaCallbacks {
    fn analyze_crate<'tcx>(&mut self, _compiler: &interface::Compiler, tcx: TyCtxt<'tcx>) {
        let crate_name = tcx.crate_name(LOCAL_CRATE).to_string();
        debug!("Analyzing crate: {}", crate_name);
        
        if tcx.sess.opts.unstable_opts.no_codegen || !tcx.sess.opts.output_types.should_codegen() {
            return;
        }
        
        // Collect all function instances
        let cgus = tcx.collect_and_partition_mono_items(()).codegen_units;
        let instances: Vec<Instance<'tcx>> = cgus
            .iter()
            .flat_map(|cgu| {
                cgu.items().iter().filter_map(|(mono_item, _)| {
                    if let MonoItem::Fn(instance) = mono_item {
                        Some(*instance)
                    } else {
                        None
                    }
                })
            })
            .collect();
        
        debug!("Analyzing {} functions", instances.len());
        
        // Process each function with DFS traversal
        let typing_env = TypingEnv::fully_monomorphized();
        for instance in instances {
            // Try to get MIR body and perform analysis
            if let Some(body) = get_mir_body(tcx, instance, typing_env) {
                analyze_function(tcx, instance, body);
            }
        }
        
        debug!("Analysis complete");
    }
}

/// Get MIR body for an instance
fn get_mir_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    _typing_env: TypingEnv<'tcx>,
) -> Option<&'tcx Body<'tcx>> {
    let def_id = instance.def_id();
    
    // Only process instances in local crate
    if def_id.krate != LOCAL_CRATE {
        return None;
    }
    
    // Additional check: Filter out external dependencies by source file path
    // This handles cases where generic instantiations or inlined functions
    // from external crates appear in the local crate's codegen units
    let span = tcx.def_span(def_id);
    if !span.is_dummy() {
        let source_file = tcx.sess.source_map().lookup_source_file(span.lo());
        
        if let Some(file_path) = source_file.name.prefer_local().to_str() {
            // Skip files from .cargo registry (external dependencies)
            if file_path.contains(".cargo") && file_path.contains("registry") {
                debug!("Skipping external dependency: {} (source: {})", 
                       tcx.def_path_str(def_id), file_path);
                return None;
            }
            
            // Skip files from target directory (build artifacts)
            if file_path.contains("\\target\\") || file_path.contains("/target/") {
                debug!("Skipping target directory file: {} (source: {})", 
                       tcx.def_path_str(def_id), file_path);
                return None;
            }
        }
    }
    
    // Get the MIR body for this instance
    Some(tcx.instance_mir(instance.def))
}

/// Analyze a function using DFS traversal with state management
fn analyze_function<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &'tcx Body<'tcx>,
) {
    let def_id = instance.def_id();
    let name = tcx.def_path_str(def_id);
    
    // Report function analysis start
    crate::report::report_function_start(&name, body);
    
    // Create a BindingManager for this function
    let mut manager = crate::state::BindingManager::new(&name);
    
    // Register all locals
    for (local_idx, _local_decl) in body.local_decls.iter_enumerated() {
        let id_str = format!("_{}", local_idx.as_usize());
        manager.register(id_str, None);
    }
    
    // Use DFS traversal with state management for branches
    use rustc_middle::mir::START_BLOCK;
    crate::dfs::dfs_visit_with_manager(
        body,
        START_BLOCK,
        &mut manager,
        &mut |bb_idx, mgr| {
            let bb = &body.basic_blocks[bb_idx];
            
            // Analyze each statement in this basic block
            for stmt in &bb.statements {
                crate::detect::detect_stmt(stmt, mgr, bb_idx, &name, body);
            }
            
            // Analyze terminator
            if let Some(ref terminator) = bb.terminator {
                crate::detect::detect_terminator(terminator, mgr, body, tcx, bb_idx, &name);
            }
        },
    );
    
    // Report function analysis end
    crate::report::report_function_end(&name);
}

