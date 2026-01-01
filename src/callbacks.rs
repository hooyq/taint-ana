//! The main functionality: callbacks for rustc plugin systems to extract function signatures.
//! Inspired by lockbud
extern crate rustc_driver;
extern crate rustc_hir;

use std::path::PathBuf;

use log::{debug, info};
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
        self.extract_function_signatures(compiler, tcx);
        // Continue compilation to allow cargo to work properly
        Compilation::Continue
    }
}

impl TaintAnaCallbacks {
    fn extract_function_signatures<'tcx>(&mut self, _compiler: &interface::Compiler, tcx: TyCtxt<'tcx>) {
        let crate_name = tcx.crate_name(LOCAL_CRATE).to_string();
        debug!("Extracting function signatures from crate: {}", crate_name);
        
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
        
        let instances_count = instances.len();
        debug!("Found {} function instances", instances_count);
        
        // Process each function: extract signature and traverse basic blocks
        let typing_env = TypingEnv::fully_monomorphized();
        for instance in instances {
            // Extract function signature
            if let Some(signature) = extract_signature(tcx, instance) {
                info!("Processing function: {}", signature);
                debug!("  Signature details: {:?}", signature);
            } else {
                // Fallback: use simple name extraction if signature extraction fails
                let def_id = instance.def_id();
                let name = tcx.def_path_str_with_args(def_id, instance.args);
                info!("Processing function: {} (signature extraction failed)", name);
            }
            
            // Try to get MIR body and traverse basic blocks
            let def_id = instance.def_id();
            if let Some(body) = get_mir_body(tcx, instance, typing_env) {
                traverse_basic_blocks(tcx, instance, &body);
            } else {
                let name = tcx.def_path_str_with_args(def_id, instance.args);
                debug!("Function {} has no MIR body", name);
            }
        }
        
        info!("=== Finished processing {} functions ===", instances_count);
    }
}

#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub name: String,
    pub inputs: Vec<String>,
    pub output: String,
    pub is_async: bool,
    pub is_unsafe: bool,
}

impl std::fmt::Display for FunctionSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let unsafe_str = if self.is_unsafe { "unsafe " } else { "" };
        let async_str = if self.is_async { "async " } else { "" };
        let inputs_str = self.inputs.join(", ");
        write!(
            f,
            "{}{}fn {}({}) -> {}",
            unsafe_str, async_str, self.name, inputs_str, self.output
        )
    }
}

/// Get MIR body for an instance
/// TODO: 完善错误处理和边界情况
fn get_mir_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    _typing_env: TypingEnv<'tcx>,
) -> Option<&'tcx Body<'tcx>> {
    // Only process instances in local crate
    if instance.def_id().krate != LOCAL_CRATE {
        return None;
    }
    
    // Get the MIR body for this instance
    Some(tcx.instance_mir(instance.def))
}

/// Traverse all basic blocks in a function
fn traverse_basic_blocks<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &'tcx Body<'tcx>,
) {
    let def_id = instance.def_id();
    let name = tcx.def_path_str(def_id);
    
    info!("  Function: {} - Found {} basic blocks", name, body.basic_blocks.len());
    
    // Create a BindingManager for this function
    let mut manager = crate::state::BindingManager::new(&name);
    
    // Register all locals
    for (local_idx, _local_decl) in body.local_decls.iter_enumerated() {
        let id_str = format!("_{}", local_idx.as_usize());
        manager.register(id_str, None);
    }
    
    // Traverse each basic block
    // TODO: Implement proper DFS traversal with state management for branches
    for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
        debug!("    BasicBlock[{:?}]:", bb_idx);
        debug!("      Statements: {}", bb.statements.len());
        
        // Analyze each statement
        for stmt in &bb.statements {
            crate::detect::detect_stmt(stmt, &mut manager, bb_idx);
        }
        
        // Analyze terminator
        if let Some(ref terminator) = bb.terminator {
            debug!("      Terminator: {:?}", &terminator.kind);
            crate::detect::detect_terminator(terminator, &mut manager, body, tcx, bb_idx);
        }
    }
}

/// Extract function signature (simplified version)
/// TODO: 完善函数签名提取
/// - 正确处理 EarlyBinder<FnSig>
/// - 提取完整的参数类型
/// - 提取返回类型
/// - 检测 unsafe 和 async
fn extract_signature<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
) -> Option<FunctionSignature> {
    let def_id = instance.def_id();
    let typing_env = TypingEnv::fully_monomorphized();
    
    // Get function name
    let name = tcx.def_path_str_with_args(def_id, instance.args);
    
    // Get the function's type from the instance
    let instance_ty = instance.ty(tcx, typing_env);
    
    // Extract function signature from the type
    // Use a simplified approach: format the type information directly
    let (inputs, output) = match instance_ty.kind() {
        rustc_middle::ty::TyKind::FnPtr(fn_sig_binder, _) => {
            // For function pointers, extract from the binder
            let fn_sig = fn_sig_binder.skip_binder();
            let inputs: Vec<String> = fn_sig.inputs()
                .iter()
                .map(|ty| format!("{:?}", ty))
                .collect();
            let output = format!("{:?}", fn_sig.output());
            (inputs, output)
        }
        _ => {
            // For FnDef and other types, use the type itself
            // The type string will contain signature information
            let type_str = format!("{:?}", instance_ty);
            // Try to extract basic info from the type string
            // For now, just use empty inputs and the type as output
            (vec![], type_str)
        }
    };
    
    // Check if function is async (generator) - simplified check
    let is_async = false; // TODO: 实现 async 检测
    
    // Check if function is unsafe - simplified check
    let is_unsafe = false; // TODO: 实现 unsafe 检测
    
    Some(FunctionSignature {
        name,
        inputs,
        output,
        is_async,
        is_unsafe,
    })
}

