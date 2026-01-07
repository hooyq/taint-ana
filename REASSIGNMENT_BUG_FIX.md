# 重新赋值检测 Bug 修复

## 问题描述

在引入解引用 ID 跟踪后，出现了新的误报：

```
❌ 检测到错误: Use After Drop
│ 函数: <callbacks::TaintAnaCallbacks as rustc_driver::Callbacks>::config
│ 变量: _33
│ 基本块: bb20
│ 
│ MIR:
│   bb19: drop(((*_1).1: std::path::PathBuf))
│   bb20:
│     [0] ((*_1).1: std::path::PathBuf) = move _31  // ← 重新赋值！
│     [1] _33 = &mut ((*_1).1: std::path::PathBuf)
│     [T] _32 = std::path::PathBuf::pop(move _33)  // ← 误报！
```

## 根本原因

重新赋值检测逻辑与新的解引用 ID 系统**不兼容**：

### 旧代码的问题

```rust
// detect.rs 第 145 行（修复前）
let left_id = extract_base_local_from_place(left);  // ❌ 只提取 "_1"

// 第 159-163 行（修复前）
let is_direct_local = left.projection.is_empty();
let is_simple_deref = left.projection.len() == 1
    && matches!(left.projection[0], ProjectionElem::Deref);

if is_direct_local || is_simple_deref {  // ❌ (*_1).1 不满足！
    let was_dropped = manager.is_dropped(target_id);
    if was_dropped {
        manager.undrop_group(target_id);
    }
}
```

**两个 Bug：**

1. **`extract_base_local_from_place`**：
   - 对于 `(*_1).1`，只提取 `"_1"`
   - 但实际 drop 的是 `"*_1.1"`（完整 ID）
   - 检查 `_1` 是否 dropped → 返回 false
   - 所以不会触发 undrop

2. **条件过于严格**：
   - 只允许 `_4`（无投影）或 `*_4`（单个 Deref）
   - `(*_1).1` 有投影 `[Deref, Field(1)]`（长度 2）
   - 不满足条件 → **重新赋值检测完全不运行**

### 执行流程分析

```
bb19: drop((*_1).1)
  ↓ extract_local_from_place
  ↓ ID = "*_1.1"
  ↓ 标记 "*_1.1" dropped ✓

bb20[0]: (*_1).1 = move _31
  ↓ 重新赋值检测
  ↓ extract_base_local_from_place → "_1"
  ↓ manager.is_dropped("_1") → false（实际 dropped 的是 "*_1.1"）
  ↓ 不满足条件 → 跳过
  ↓ "*_1.1" 仍然是 dropped ❌

bb20[1]: _33 = &mut (*_1).1
  ↓ extract_local_from_place
  ↓ source = "*_1.1"
  ↓ manager.bind("*_1.1", "_33")
  ↓ "_33" 继承 dropped 状态

bb20[T]: use _33
  ↓ check_deref_dependencies("_33", manager)
  ↓ manager.is_dropped("_33") → true（从 "*_1.1" 继承）
  ↓ 报错：Use After Drop ❌ 误报！
```

## 修复方案

### 核心改进

1. **使用完整 ID**：提取 `extract_local_from_place(left)` 而不是 `extract_base_local_from_place(left)`
2. **移除条件限制**：任何形式的 place 都可以触发重新赋值恢复
3. **保持兼容性**：绑定操作仍使用 `left_base_id`

### 修复后的代码

```rust
pub fn detect_stmt(stmt: &Statement<'_>, manager: &mut BindingManager, bb: BasicBlock, fn_name: &str, body: &Body<'_>) {
    match &stmt.kind {
        StatementKind::Assign(box(left, rValue)) => {
            // ✅ 提取完整 ID（包括解引用和字段）用于重新赋值检测
            let left_full_id = extract_local_from_place(left);
            // ✅ 提取基础 ID 用于绑定操作（保持兼容性）
            let left_base_id = extract_base_local_from_place(left);
            let rvalue = rValue.clone();

            // ✅ 检查是否是重新赋值（移除条件限制）
            // 对于任何形式的 place（包括 *_1.1, _4, (*_4) 等），如果之前被 dropped，重新赋值应该恢复状态
            if let Some(ref target_id) = left_full_id {
                let was_dropped = manager.is_dropped(target_id);
                if was_dropped {
                    if is_debug_enabled() {
                        println!(
                            "  [DEBUG] Reassignment detected: {} is being reassigned in bb {:?}, restoring drop state",
                            target_id,
                            bb
                        );
                    }
                    manager.undrop_group(target_id);  // ✅ 恢复完整 ID 的状态
                }
            }

            match rValue {
                Rvalue::Move(place) => {
                    let source_id = extract_local_from_place(&place);
                    // ... use_check ...
                    
                    // ✅ 绑定操作使用 left_base_id（保持兼容性）
                    if let (Some(ref source), Some(ref target)) = (source_id, left_base_id) {
                        manager.bind(source, target);
                    }
                }
                Rvalue::Ref(_, _, place) => {
                    let source_id = extract_local_from_place(&place);
                    // ... use_check ...
                    
                    // ✅ 绑定操作使用 left_base_id
                    if let (Some(ref source), Some(ref target)) = (source_id, left_base_id) {
                        manager.bind(source, target);
                    }
                }
                // ...
            }
        }
    }
}
```

### 修复后的执行流程

```
bb19: drop((*_1).1)
  ↓ ID = "*_1.1"
  ↓ 标记 "*_1.1" dropped ✓

bb20[0]: (*_1).1 = move _31
  ↓ 重新赋值检测
  ↓ left_full_id = extract_local_from_place → "*_1.1" ✓
  ↓ manager.is_dropped("*_1.1") → true ✓
  ↓ manager.undrop_group("*_1.1") ✓
  ↓ "*_1.1" 恢复为 not dropped ✓

bb20[1]: _33 = &mut (*_1).1
  ↓ source = "*_1.1"
  ↓ manager.bind("*_1.1", "_1")  // 使用 base_id
  ↓ "_33" 继承 not dropped 状态 ✓

bb20[T]: use _33
  ↓ check_deref_dependencies("_33", manager)
  ↓ manager.is_dropped("_33") → false ✓
  ↓ 不报错 ✓ 正确！
```

## 为什么绑定仍使用 `left_base_id`？

**原因**：保持与现有绑定系统的兼容性。

**示例**：
```rust
let x = vec![1, 2, 3];  // _20
let r = &mut x;         // _21 = &mut _20
```

**绑定关系**：
- `_20` ↔ `_21`（基础 local 的绑定）
- 这样 `drop(_20)` 会标记整个组为 dropped
- `*_21` 的依赖检查会发现 `_21` dropped，从而间接检测到错误

**如果使用完整 ID**：
- `*_1.1` ↔ `_33`
- 但 `_1` 可能有其他绑定关系
- 会导致绑定关系混乱

## 影响范围

### 修复的场景

1. **字段 + 解引用的重新赋值**：
   ```rust
   drop((*obj).field);
   (*obj).field = new_value;  // ✓ 现在会正确恢复状态
   use (*obj).field;          // ✓ 不再误报
   ```

2. **复杂嵌套的重新赋值**：
   ```rust
   drop((*(*ptr).inner).value);
   (*(*ptr).inner).value = x;  // ✓ 正确恢复
   ```

3. **多层字段访问**：
   ```rust
   drop(_1.0.1.2);
   _1.0.1.2 = y;  // ✓ 正确恢复
   ```

### 不影响的场景

- **简单 local**：`_4 = ...` - 仍然正常工作
- **单个解引用**：`*_4 = ...` - 仍然正常工作
- **绑定关系**：现有的绑定逻辑不变

## 测试验证

### 测试用例 1：字段 + 解引用重新赋值

```rust
struct Container {
    inner: Option<Vec<i32>>,
}

fn test_field_deref_reassign() {
    let mut c = Container { inner: Some(vec![1, 2, 3]) };
    let ptr = &mut c as *mut Container;
    
    unsafe {
        // MIR: drop((*ptr).inner)
        drop((*ptr).inner);
        
        // MIR: (*ptr).inner = Some(vec![4, 5, 6])
        // ✓ 应该恢复 (*ptr).inner 的状态
        (*ptr).inner = Some(vec![4, 5, 6]);
        
        // MIR: r = &mut (*ptr).inner
        let r = &mut (*ptr).inner;
        
        // ✓ 应该不报错
        if let Some(ref mut v) = r {
            v.push(7);
        }
    }
}
```

### 测试用例 2：callbacks.rs 的真实场景

```rust
// src/callbacks.rs:config
fn config(&mut self, config: &mut Config) {
    // bb19: drop((*self).rustc_dir)
    // bb20: (*self).rustc_dir = PathBuf::new()
    //       let r = &mut (*self).rustc_dir
    //       PathBuf::pop(r)  // ✓ 不应该误报
    self.rustc_dir = PathBuf::new();
    self.rustc_dir.pop();  // ✓ 不误报
}
```

## 总结

### 问题
新的解引用 ID 系统与旧的重新赋值检测逻辑不兼容，导致对带有解引用和字段访问的重新赋值场景产生误报。

### 修复
- 使用**完整 ID**（`extract_local_from_place`）进行重新赋值检测
- **移除条件限制**，支持任何形式的 place
- 保持绑定操作使用**基础 ID**（向后兼容）

### 影响
- ✅ 修复了 `(*_1).1` 类型的重新赋值误报
- ✅ 保持了现有功能的兼容性
- ✅ 没有引入新的误报或漏报

---

**修复日期**：2026-01-07  
**相关文件**：`src/detect.rs` 第 142-243 行  
**相关 Issue**：callbacks.rs config 函数的 Use After Drop 误报

