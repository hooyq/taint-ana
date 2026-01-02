use std::collections::HashSet;
use std::sync::OnceLock;

use rustc_middle::mir::{Body, Operand, Place, ProjectionElem, Rvalue, Statement, StatementKind, Terminator, TerminatorKind, BasicBlock, PlaceElem};
use rustc_middle::ty::{TyCtxt, TyKind};
use rustc_span::Symbol;
use rustc_index::Idx;

use crate::state::BindingManager;

/// 从 Place 提取基础 local ID（String 格式，如 "_1"）
fn extract_base_local_from_place(place: &Place) -> Option<String> {
    let local_usize: usize = place.local.as_usize();
    Some(format!("_{}", local_usize))
}

/// 从 Place 提取完整的 local ID（String 格式，支持多层嵌套）
///
/// 示例：
/// - `_1` → "_1"
/// - `_1.0` → "_1.0" (结构体字段)
/// - `_1.3.4.5` → "_1.3.4.5" (嵌套结构体字段)
/// - `(_1 as Some).0` → "(_1 as 0).0" (枚举字段，variant 0, field 0)
/// - `((_1.0) as Some).0` → "((_1.0) as 0).0" (结构体字段中的枚举字段)
/// - `(*_1.0)` → "_1.0" (Deref 之前的字段)
/// - `(_1.0)[_2]` → "_1.0" (Index 之前的字段)
///
/// 策略：
/// - Field: 追加 ".{field_index}"
/// - Downcast + Field: 追加 " as {variant_index}).{field_index}"
/// - Deref/Index/ConstantIndex/Subslice: 停止处理（返回当前构建的 ID）
/// - 其他: 停止处理
fn extract_local_from_place(place: &Place) -> Option<String> {
    let base_local = extract_base_local_from_place(place)?;

    let projection = &place.projection;

    if projection.is_empty() {
        return Some(base_local);
    }

    let mut current_id = base_local;
    let mut i = 0;

    while i < projection.len() {
        match &projection[i] {
            ProjectionElem::Downcast(_, variant_idx) => {
                // 检查下一个元素是否是 Field
                if i + 1 < projection.len() {
                    if let ProjectionElem::Field(field_idx, _) = &projection[i + 1] {
                        // 找到 Downcast + Field，这是枚举字段访问
                        let variant_index = variant_idx.as_usize();
                        let field_index = field_idx.as_usize();
                        // 追加 " as {variant_index}).{field_index}"
                        current_id = format!("({} as {}).{}", current_id, variant_index, field_index);
                        i += 2;  // 跳过 Downcast 和 Field
                        continue;
                    }
                }
                // Downcast 后面没有 Field，停止处理
                break;
            }
            ProjectionElem::Field(field_idx, _) => {
                // 检查前面是否有 Downcast（在同一位置）
                if i > 0 {
                    if let ProjectionElem::Downcast(_, _) = &projection[i - 1] {
                        // 前面有 Downcast，这是枚举字段，应该在上一次迭代中处理
                        i += 1;
                        continue;
                    }
                }
                // 单独的 Field（没有前面的 Downcast），这是结构体字段
                let field_index = field_idx.as_usize();
                // 追加 ".{field_index}"
                current_id = format!("{}.{}", current_id, field_index);
                i += 1;
            }
            ProjectionElem::Deref => {
                // Deref 之前可能有字段访问，已经处理了
                // Deref 之后停止处理
                break;
            }
            ProjectionElem::Index(_) |
            ProjectionElem::ConstantIndex { .. } |
            ProjectionElem::Subslice { .. } => {
                // Index 之前可能有字段访问，已经处理了
                // Index 之后停止处理
                break;
            }
            ProjectionElem::OpaqueCast(_) => {
                // OpaqueCast 不影响字段路径，继续处理
                i += 1;
            }

            //ProjectionElem::Subtype(_)=>{}

            PlaceElem::UnwrapUnsafeBinder(_) => {}


        }
    }

    Some(current_id)
}

/// 提取 Operand 中的 local ID（String 格式）
fn extract_local_from_operand(operand: &Operand) -> Option<String> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            extract_local_from_place(place)
        }
        Operand::Constant(_) => None,
    }
}

/// 提取 Operand 中的基础 local ID（String 格式）
/// 用于需要基础 local 的场景（如 use_check，需要检查基础 local 是否已 drop）
fn extract_base_local_from_operand(operand: &Operand) -> Option<String> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            extract_base_local_from_place(place)
        }
        Operand::Constant(_) => None,
    }
}

/// 全局黑名单（懒加载，只读取一次）
static BLACKLIST: OnceLock<HashSet<String>> = OnceLock::new();

pub fn detect_stmt(stmt: &Statement<'_>, manager: &mut BindingManager, bb: BasicBlock, fn_name: &str, body: &Body<'_>) {
    match &stmt.kind {
        StatementKind::Assign(box(left, rValue)) => {
            let left_id = extract_base_local_from_place(left);
            let rvalue = rValue.clone();

            // 检查是否是重新赋值（左值是 local，没有字段访问）
            // 如果是重新赋值，应该恢复 local 的 drop 状态
            // 这解决了 MIR 中 drop 后立即重新赋值的问题（如 *manager = saved_state.clone()）
            // 关键：必须在检查右值 use 之前恢复状态，否则 use_check 会误报
            if let Some(ref target_id) = left_id {
                // 检查左值是否是：
                // 1. 直接的 local（没有 projection），例如 `_4 = ...`
                // 2. 只有一个 Deref 的投影，例如 `(*_4) = ...`
                //
                // 对于第二种情况，本质上也是"通过引用重新初始化这个 group 对应的值"，
                // 对我们的抽象来说等价于"重新赋值 local 4"，应该恢复 drop 状态。
                let is_direct_local = left.projection.is_empty();
                let is_simple_deref = left.projection.len() == 1
                    && matches!(left.projection[0], ProjectionElem::Deref);

                if is_direct_local || is_simple_deref {
                    // 如果 local 被 drop 了，任何赋值都可能是重新赋值
                    // 这包括 Rvalue::Use（从其他值复制/移动）和其他类型的赋值
                    let was_dropped = manager.is_dropped(target_id);
                    if was_dropped {
                        // 这是重新赋值，恢复 local 的 drop 状态
                        if is_debug_enabled() {
                            println!(
                                "  [DEBUG] Reassignment detected: local {} is being reassigned in bb {:?}, restoring drop state (direct={}, deref={})",
                                target_id,
                                bb,
                                is_direct_local,
                                is_simple_deref
                            );
                        }
                        manager.undrop_group(target_id);
                    }
                }
            }
            match rValue {
                Rvalue::Use(op) => {
                    match op {
                        Operand::Copy(place) => {
                            // Copy 操作：检查基础 local（因为需要读取枚举的判别值）
                            // 注意：如果这是重新赋值的一部分（左值刚被恢复状态），
                            // 右值的 use_check 应该在重新赋值检测之后，所以这里应该没问题
                            let base_id = extract_base_local_from_place(&place);
                            use_check_stmt(base_id, manager, stmt, bb, fn_name, body);
                        }
                        Operand::Move(place) => {
                            // Move 操作：提取 local ID（支持多层嵌套）
                            let source_id = extract_local_from_place(&place);
                            
                            // 对于 use_check，需要检查基础 local（因为读取枚举字段需要读取枚举的判别值）
                            // 注意：如果这是重新赋值的一部分（左值刚被恢复状态），
                            // 右值的 use_check 应该在重新赋值检测之后，所以这里应该没问题
                            let base_id = extract_base_local_from_place(&place);
                            use_check_stmt(base_id.clone(), manager, stmt, bb, fn_name, body);
                            
                            // 确保 source_id 已注册
                            if let Some(ref source) = source_id {
                                manager.register(source.clone(), None);
                            }
                            
                            // Move 操作：绑定源变量和目标变量
                            if let (Some(ref source), Some(ref target)) = (source_id, left_id) {
                                if is_debug_enabled() {
                                    let source_dropped_before = manager.is_dropped(source);
                                    let target_dropped_before = manager.is_dropped(target);
                                    println!("  [DEBUG] Move: binding {} -> {} (source_dropped={}, target_dropped={})", 
                                        source, target, source_dropped_before, target_dropped_before);
                                }
                                
                                if let Err(e) = manager.bind(source, target) {
                                    eprintln!("⚠️  Warning: bind failed in Move {} -> {}: {}", source, target, e);
                                } else if is_debug_enabled() {
                                    let source_dropped_after = manager.is_dropped(source);
                                    let target_dropped_after = manager.is_dropped(target);
                                    println!("    [DEBUG] After bind: source_dropped={}, target_dropped={}", 
                                        source_dropped_after, target_dropped_after);
                                }
                            }
                        }
                        Operand::Constant(_) => {}
                    }
                }
                Rvalue::Repeat(op, _) => {
                    // Repeat: use op (e.g., [x; 3]，重复 use x)
                    // 可能涉及字段访问，使用 extract 更精确
                    let id_opt = extract_local_from_operand(&op);
                    use_check_stmt(id_opt, manager, stmt, bb, fn_name, body);
                }
                Rvalue::Ref(_, _, place) => {
                    // Ref: use place (借用，读取 source)
                    // 对于 use_check，需要检查基础 local
                    let base_id = extract_base_local_from_place(&place);
                    use_check_stmt(base_id.clone(), manager, stmt, bb, fn_name, body);
                    
                    // 提取 local ID（支持多层嵌套）
                    let source_id = extract_local_from_place(&place);
                    
                    // 确保 source_id 已注册
                    if let Some(ref source) = source_id {
                        manager.register(source.clone(), None);
                    }
                    
                    // 绑定引用源和目标
                    if let (Some(ref source), Some(ref target)) = (source_id, left_id) {
                        if let Err(e) = manager.bind(source, target) {
                            eprintln!("⚠️  Warning: bind failed in Ref {} -> {}: {}", source, target, e);
                        }
                    }
                }
                Rvalue::ThreadLocalRef(_) => {
                    // ThreadLocalRef: 无 local use (全局线程本地)
                }
                Rvalue::RawPtr(_, place) => {
                    // RawPtr: 获取原始指针
                    let id_opt = extract_local_from_place(&place);
                    use_check_stmt(id_opt, manager, stmt, bb, fn_name, body);
                }
                Rvalue::Cast(_, op, _) => {
                    // Cast: use op (e.g., a = b as i32)
                    // 可能涉及字段访问，使用 extract 更精确
                    let id_opt = extract_local_from_operand(&op);
                    use_check_stmt(id_opt, manager, stmt, bb, fn_name, body);
                }
                Rvalue::BinaryOp(_, box (op1, op2)) => {
                    // BinaryOp (e.g., a = b + c): use op1 和 op2
                    // 可能涉及字段访问，使用 extract 更精确
                    let id1_opt = extract_local_from_operand(&op1);
                    use_check_stmt(id1_opt, manager, stmt, bb, fn_name, body);
                    let id2_opt = extract_local_from_operand(&op2);
                    use_check_stmt(id2_opt, manager, stmt, bb, fn_name, body);
                }
                Rvalue::NullaryOp(_, _) => {
                    // NullaryOp (e.g., BoxNew, Null): 无 Operand/Place use
                }
                Rvalue::UnaryOp(_, op) => {
                    // UnaryOp (e.g., a = -b): use op
                    // 可能涉及字段访问，使用 extract 更精确
                    let id_opt = extract_local_from_operand(&op);
                    use_check_stmt(id_opt, manager, stmt, bb, fn_name, body);
                }
                Rvalue::Discriminant(place) => {
                    // Discriminant: use place (enum 标签)
                    let id_opt = extract_base_local_from_place(&place);
                    use_check_stmt(id_opt, manager, stmt, bb, fn_name, body);
                }
                Rvalue::Aggregate(_, fields) => {
                    // Aggregate (struct/tuple/array init): fields 是 Vec<Operand>，每个可能 use
                    // 可能涉及字段访问，使用 extract 更精确
                    for field in fields {
                        let id_opt = extract_local_from_operand(&field);
                        use_check_stmt(id_opt, manager, stmt, bb, fn_name, body);
                    }
                }
                Rvalue::ShallowInitBox(op, _) => {
                    // ShallowInitBox: use op (box init)
                    // 可能涉及字段访问，使用 extract 更精确
                    let id_opt = extract_local_from_operand(&op);
                    use_check_stmt(id_opt, manager, stmt, bb, fn_name, body);
                }
                Rvalue::CopyForDeref(place) => {
                    // CopyForDeref: use place (解引用 copy)
                    // 可能涉及字段访问，使用 extract 更精确
                    let id_opt = extract_local_from_place(&place);
                    use_check_stmt(id_opt, manager, stmt, bb, fn_name, body);
                }
                Rvalue::WrapUnsafeBinder(op, _) => {
                    // WrapUnsafeBinder: 包装不安全的 binder
                    let id_opt = extract_local_from_operand(&op);
                    use_check_stmt(id_opt, manager, stmt, bb, fn_name, body);
                }
            }
        }
        StatementKind::FakeRead(_) => {}
        StatementKind::SetDiscriminant { .. } => {}
        StatementKind::StorageLive(_) => {}
        StatementKind::StorageDead(_) => {}
        StatementKind::Retag(_, _) => {}
        StatementKind::PlaceMention(_) => {}
        StatementKind::AscribeUserType(_, _) => {}
        StatementKind::Coverage(_) => {}
        StatementKind::Intrinsic(_) => {}
        StatementKind::ConstEvalCounter => {}
        StatementKind::Nop => {}
        StatementKind::BackwardIncompatibleDropHint { .. } => {}
        //StatementKind::Deinit(_) => {}
    }
}

/// 调试标志：是否输出详细调试信息（可通过环境变量 DEBUG_MIR=1 控制）
fn is_debug_enabled() -> bool {
    std::env::var("DEBUG_MIR").is_ok()
}

/// 统一的 use 检查函数（用于 Statement）
/// 检查变量是否已被 drop，如果已 drop 则返回错误并打印 span
pub fn use_check_stmt(id_opt: Option<String>, manager: &mut BindingManager, stmt: &Statement<'_>, bb: BasicBlock, fn_name: &str, body: &Body<'_>) -> Result<(), String> {
    if let Some(ref id) = id_opt {
        // 确保已注册
        manager.register(id.clone(), None);

        // 检查是否被 drop
        if is_debug_enabled() {
            let dropped = manager.is_dropped(id);
            println!("  [DEBUG] use_check_stmt: local {} at {:?}, dropped={}", id, stmt.source_info.span, dropped);
            if dropped {
                // 打印当前状态信息
                if let Some((root_id, members)) = manager.find_group(id) {
                    println!("    [DEBUG] Group root: {}, members: {:?}", root_id, members);
                }
            }
        }

        if manager.is_dropped(id) {
            // 使用新的报告函数
            crate::report::report_use_after_drop_stmt(fn_name, stmt, bb, id, body, manager);
            return Err(format!("Use after drop: {}", id));
        }
    }
    Ok(())
}

/// 统一的 use 检查函数（用于 Terminator）
/// 检查变量是否已被 drop，如果已 drop 则返回错误并打印 span
pub fn use_check_term(id_opt: Option<String>, manager: &mut BindingManager, term: &Terminator<'_>, bb: BasicBlock, fn_name: &str, body: &Body<'_>) -> Result<(), String> {
    if let Some(ref id) = id_opt {
        // 确保已注册
        manager.register(id.clone(), None);

        if manager.is_dropped(id) {
            // 使用新的报告函数
            crate::report::report_use_after_drop_term(fn_name, term, bb, id, body, manager);
            return Err(format!("Use after drop: {}", id));
        }
    }
    Ok(())
}

pub fn detect_terminator<'tcx>(
    term: &Terminator<'tcx>,
    manager: &mut BindingManager,
    body: &Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    bb: BasicBlock,
    fn_name: &str
) {
    match &term.kind {
        TerminatorKind::Goto { .. } => {
            // Goto: 无条件跳转，不涉及 use/drop
        }
        TerminatorKind::SwitchInt { discr, .. } => {
            // SwitchInt: 基于整数值的条件跳转，discr 被使用
            let id_opt = extract_base_local_from_operand(discr);
            use_check_term(id_opt, manager, term, bb, fn_name, body);
        }
        TerminatorKind::UnwindResume => {
            // UnwindResume: 异常恢复，不涉及 use/drop
        }
        TerminatorKind::UnwindTerminate(_) => {
            // UnwindTerminate: 程序终止，不涉及 use/drop
        }
        TerminatorKind::Return => {
            // Return: 函数返回
            // 检查返回值（通常存储在 local 0，即 _0）是否已被 drop
            // 返回值在返回时会被 move，所以需要检查它是否已被 drop
            // 注意：返回值总是存储在 local 0（_0）
            let return_id = Some("_0".to_string());  // 返回值存储在 local 0
            use_check_term(return_id, manager, term, bb, fn_name, body);
        }
        TerminatorKind::Unreachable => {
            // Unreachable: 不可达代码，不涉及 use/drop
        }
        TerminatorKind::Drop { place, target: _, unwind: _, replace: _, .. } => {
            // Drop terminator：直接调用 drop_check，让它统一处理所有情况
            let id = extract_local_from_place(place);

            if is_debug_enabled() {
                if let Some(ref id_val) = id {
                    let dropped_before = manager.is_dropped(id_val);
                    println!("  [DEBUG] Drop terminator: local {} at {:?}, dropped_before={}",
                             id_val, term.source_info.span, dropped_before);
                }
            }

            // 直接调用 drop_check，让它处理所有情况（包括 double drop 检测）
            drop_check(id, manager, term, bb);
        }
        TerminatorKind::Call { func, args, destination, .. } => {
            let ty = func.ty(body, tcx);

            if let TyKind::FnDef(def_id, _args) = ty.kind() {
                let name = tcx.item_name(*def_id);

                // 检查函数名是否包含 "::drop"（如 std::mem::drop）
                // 如果包含，将这个函数调用视为 drop 操作
                let name_str = name.as_str();
                let is_drop_function = name_str.contains("::drop");

                if is_drop_function && !args.is_empty() {
                    // 提取第一个参数（通常是 Operand::Move）
                    let arg = &args[0];
                    let arg_id = extract_local_from_operand(&arg.node);

                    if let Some(ref id_str) = arg_id {
                        if is_debug_enabled() {
                            let dropped_before = manager.is_dropped(id_str);
                            println!("  [DEBUG] Drop function call: local {} at {:?}, dropped_before={}",
                                     id_str, term.source_info.span, dropped_before);
                        }

                        // 直接调用 drop_check，让它统一处理所有情况（包括 double drop 检测）
                        if let Err(e) = drop_check(arg_id.clone(), manager, term, bb) {
                            eprintln!("⚠️  Warning: drop_check failed in Call: {}", e);
                        }
                    }
                }

                // 使用黑名单检查函数名
                let blacklist = get_blacklist();
                if is_in_blacklist(name, blacklist) {
                    if !args.is_empty() {
                        if let (Some(dest_id), Some(arg_id)) = (
                            extract_local_from_place(destination),
                            extract_local_from_operand(&args[0].node)
                        ) {
                            // 确保 destination 和 arg 都已注册（支持字段访问）
                            manager.register(dest_id.clone(), None);
                            manager.register(arg_id.clone(), None);

                            if let Err(e) = manager.bind(&dest_id, &arg_id) {
                                eprintln!("⚠️  Warning: bind failed in Call {} -> {}: {}", dest_id, arg_id, e);
                            }
                        }
                    }
                    println!("func name in blacklist: {:?}", name);
                }

                // 检查函数调用参数
                // 注意：对于引用参数（如 &mut T），我们检查的是引用指向的 local
                // 如果这个 local 刚被重新赋值，它应该已经被恢复状态了
                // 对于 drop 函数，第一个参数已经在上面处理过了，这里只检查其他参数
                for (idx, arg) in args.iter().enumerate() {
                    // 如果是 drop 函数且是第一个参数，已经处理过了，跳过
                    if is_drop_function && idx == 0 {
                        continue;
                    }
                    let place = extract_base_local_from_operand(&arg.node);
                    // 在检查之前，确保状态是最新的
                    // 如果这个 local 在同一个基本块中被重新赋值，状态应该已经恢复了
                    use_check_term(place, manager, term, bb, fn_name, body);
                }
            }
        }
        TerminatorKind::Assert { cond, .. } => {
            // Assert: 断言检查，cond 被使用
            let id_opt = extract_base_local_from_operand(cond);
            use_check_term(id_opt, manager, term, bb, fn_name, body);
        }
        TerminatorKind::InlineAsm { .. } => {
            // InlineAsm: 内联汇编，需要检查所有操作数
            // TODO: 实现内联汇编参数检查
        }
        TerminatorKind::Yield { .. } => {
            // Yield: 生成器 yield，暂不处理
        }
        TerminatorKind::FalseEdge { .. } => {
            // FalseEdge: 用于借用检查，不涉及 use/drop
        }
        TerminatorKind::FalseUnwind { .. } => {
            // FalseUnwind: 用于借用检查，不涉及 use/drop
        }
        TerminatorKind::CoroutineDrop => {
            // CoroutineDrop: 协程 drop，暂不处理
        }
        TerminatorKind::TailCall { .. } => {}
    }
}

/// 检查 ID 是否是字段访问（如 _1.0, _1.1, (_1 as 0).0）
fn is_field_access(id: &str) -> bool {
    // 字段访问的特征：包含 "." 或 "("（枚举字段）
    id.contains('.') || id.contains('(')
}

fn drop_check(id_opt: Option<String>, manager: &mut BindingManager, terminator: &Terminator<'_>, _bb: BasicBlock) -> Result<(), String> {
    if let Some(ref id) = id_opt {
        // 确保已注册
        manager.register(id.clone(), None);

        // 直接获取该 local 的 state，检查它的 drop state
        // 这样可以区分是否是 drop 完全同一个 local（cleanup 路径中的正常行为）
        if let Some(state) = manager.states.get(id) {
            // 检查该 local 本身的 drop state（不是通过绑定关系传播的）
            if state.is_dropped {
                // 这是 drop 完全同一个 local，可能是 cleanup 路径中的正常行为，允许
                if is_debug_enabled() {
                    println!("  [DEBUG] Allow drop: local {} is already dropped (same local, possibly cleanup path)", id);
                }
                // 允许，不报错
                return Ok(());
            }
        }

        // 检查是否通过绑定关系已经被 drop（这是真正的 double drop）
        // 需要先压缩路径，然后检查 root 的 drop state
        let (root_id, path) = match crate::state::LocalState::find_root_from_id(id, &manager.states) {
            Some(p) => p,
            None => {
                // 如果找不到 root，说明还没有绑定关系，直接 drop
                manager.idrop_group(id);
                return Ok(());
            }
        };

        // 压缩路径
        crate::state::LocalState::compress_path(&mut manager.states, &path, &root_id);

        // 检查 root 的 drop state
        if crate::state::LocalState::get_root_dropped(&root_id, &manager.states) {
            // 通过绑定关系已经被 drop
            // 如果 root_id == id，说明是 drop 同一个 local，允许（可能是 cleanup 路径）
            if root_id == *id {
                if is_debug_enabled() {
                    println!("  [DEBUG] Allow drop: local {} is root and already dropped (possibly cleanup path)", id);
                }
                return Ok(());
            }

            // root_id != id，说明是通过绑定关系传播的
            // 关键问题：如果该 local 通过绑定关系已经被 drop，再次 drop 可能是 cleanup 路径的正常行为
            // 但是，如果该 local 本身也被 drop 了（state.is_dropped == true），那么应该允许
            // 如果该 local 本身没有被 drop（state.is_dropped == false），但 root 被 drop 了，
            // 这可能是误报，因为该 local 和 root 是同一个值（通过绑定关系）
            // 
            // 但是，我们无法区分是否是 cleanup 路径，所以这里应该允许
            // 因为：如果 _7 被绑定到 (_5 as 1).0，当 (_5 as 1).0 被 drop 时，_7 也应该被视为已 drop
            // 在 cleanup 路径中再次 drop _7 是正常的 MIR 行为
            // 
            // 注意：这可能会漏掉一些真正的 double drop，但根据错误信息，这是误报
            if is_debug_enabled() {
                println!("  [DEBUG] Allow drop: local {} is already dropped through binding (root: {}), possibly cleanup path", id, root_id);
                if let Some((r_id, members)) = manager.find_group(id) {
                    println!("   [DEBUG] Group root: {}, members: {:?}", r_id, members);
                }
            }
            return Ok(());
        }

        if is_debug_enabled() {
            if let Some((r_id, members)) = manager.find_group(id) {
                println!("  [DEBUG] drop_group: local {} -> root {}, members: {:?}", id, r_id, members);
            }
        }

        manager.idrop_group(id);
    } else {
        return Err(format!("id not found in {:?}", terminator));
    }
    Ok(())
}

//BlackList-----
/// 获取黑名单（硬编码在代码中）
/// 包含所有需要特殊处理的函数名子串
fn get_blacklist() -> &'static HashSet<String> {
    BLACKLIST.get_or_init(|| {
        let mut blacklist = HashSet::new();
        
        // 原始指针操作
        blacklist.insert("as_mut_ptr".to_string());
        blacklist.insert("as_ptr".to_string());
        
        // 引用转换
        blacklist.insert("as_ref".to_string());
        blacklist.insert("as_mut".to_string());
        
        // 原始指针构造
        blacklist.insert("from_raw_parts".to_string());
        blacklist.insert("into_raw".to_string());
        blacklist.insert("from_raw".to_string());
        blacklist.insert("_as_raw".to_string());
        
        // 解引用操作
        blacklist.insert("::deref".to_string());
        
        blacklist
    })
}

/// 检查函数名是否包含黑名单中的任何子串
fn is_in_blacklist(name: Symbol, blacklist: &HashSet<String>) -> bool {
    let name_str = name.as_str();
    blacklist.iter().any(|pattern| name_str.contains(pattern))
}
//-----

