# 解引用 ID 跟踪实现总结

## 概述

本次实现扩展了 ID 系统，将解引用（如 `*_21`）作为独立的 ID 进行跟踪，类似于字段访问（如 `_1.0`），从而精确区分指针和指针指向的内容，**消除了之前保守策略的漏报问题**。

## 核心变更

### 1. ID 命名规则（新增）

| MIR Projection | 旧 ID | 新 ID | 说明 |
|----------------|-------|-------|------|
| `[Deref]` | `_21` | `*_21` | 解引用现在有独立 ID |
| `[Deref, Deref]` | `_21` | `**_21` | 多层解引用 |
| `[Deref, Field(0)]` | `_21` | `*_21.0` | 解引用后字段访问 |
| `[Field(0), Deref]` | `_21.0` | `_21.0@deref` | 字段访问后解引用 |

### 2. 修改的文件

#### `src/detect.rs`

**新增功能：**
1. `extract_local_from_place` 现在处理 `Deref` projection
   - 前置解引用用 `*` 前缀（如 `*_21`）
   - 后置解引用用 `@deref` 后缀（如 `_21.0@deref`）

2. `check_deref_dependencies` 新函数
   - 检查解引用的依赖关系
   - 例如：使用 `*_21` 时，检查 `_21` 和 `*_21` 都有效

3. `use_check_stmt` 和 `use_check_term` 增强
   - 集成依赖检查
   - 对解引用进行递归验证

**删除功能：**
1. `has_deref_projection` 函数（不再需要）
2. `TerminatorKind::Drop` 中的跳过 Deref 逻辑
3. `TerminatorKind::Call` (drop 函数) 中的跳过 Deref 逻辑

**修改前（保守策略）：**
```rust
drop((*_21))  
   ↓
跳过跟踪（避免误报）
   ↓
后续使用 *_21 → 不报错（漏报）❌
```

**修改后（精确跟踪）：**
```rust
drop((*_21))  
   ↓
标记 "*_21" 为 dropped
   ↓
后续使用 *_21 → 报错 ✓
```

#### `src/toys/deref_tracking_test.rs`（新文件）

新增测试用例，覆盖：
1. 基本解引用跟踪
2. 指针 vs 解引用的区分
3. 依赖检查
4. 静态变量（确保不误报）
5. 多层解引用
6. 字段与解引用的组合

## 预期效果对比

### 场景 1：堆指针解引用（✅ 修复漏报）

```rust
let mut v = vec![1, 2, 3];
let ptr = &mut v as *mut Vec<i32>;
unsafe {
    drop(*ptr);  // MIR: drop((*_3))
    let x = *ptr; // ❌ Use After Free
}
```

- **修复前**：不报错（漏报）❌
- **修复后**：报错 `Use after drop: *_3` ✓

### 场景 2：静态指针（✅ 仍不误报）

```rust
static mut GLOBAL: Option<Vec<i32>> = None;
unsafe {
    GLOBAL = Some(vec![1, 2, 3]);
    let ptr = std::ptr::addr_of_mut!(GLOBAL);
    drop(*ptr);  // drop 静态变量的内容
    let p = ptr; // ✓ 使用指针本身，合法
}
```

- **修复前**：不报错 ✓
- **修复后**：不报错 ✓

### 场景 3：指针 drop 后解引用（✅ 依赖检查）

```rust
let mut v = vec![1, 2, 3];
let r = &mut v;
drop(r);
let ptr = r as *mut Vec<i32>;
unsafe {
    let x = *ptr; // ❌ 指针无效，解引用错误
}
```

- **修复前**：可能不报错（取决于绑定关系）
- **修复后**：报错 `Cannot dereference *ptr: base pointer ptr is dropped` ✓

## 技术亮点

### 1. 统一的投影抽象

解引用现在被视为一种特殊的投影，与字段访问、枚举 downcast 一样，都是 MIR Place 的组成部分：

```rust
ProjectionElem::Deref => {
    // 前置解引用：添加 * 前缀
    if current_id == base_local || current_id.starts_with('*') {
        current_id = format!("*{}", current_id);
    } else {
        // 后置解引用：添加 @deref 后缀
        current_id = format!("{}@deref", current_id);
    }
    i += 1;
}
```

### 2. 依赖检查算法

```rust
fn check_deref_dependencies(id: &str, manager: &BindingManager) -> Result<(), Vec<String>> {
    // 1. 检查前置解引用的基础指针
    if id.starts_with('*') {
        let base = id.trim_start_matches('*');
        let pure_base = extract_pure_base(base);
        if manager.is_dropped(pure_base) {
            return Err("Cannot dereference: base pointer is dropped");
        }
    }
    
    // 2. 检查后置解引用的基础部分
    if id.contains("@deref") {
        let base_part = id.split("@deref").next().unwrap();
        if manager.is_dropped(base_part) {
            return Err("Cannot dereference: base is dropped");
        }
    }
    
    // 3. 检查 ID 本身
    if manager.is_dropped(id) {
        return Err("Use after drop");
    }
    
    Ok(())
}
```

### 3. 不需要指针别名分析

本实现**不依赖复杂的指针别名分析**，而是通过：
- ID 系统的扩展（独立跟踪 `*ptr`）
- 依赖检查（`*ptr` 依赖 `ptr`）
- 现有的绑定机制（`ptr` 绑定到 `original`）

这种组合已经能够检测大多数的 use-after-free 错误。

## 权衡与限制

### 优点 ✅
1. **精确跟踪**：能检测 `drop(*ptr)` 后使用 `*ptr` 的错误
2. **统一抽象**：Deref 和 Field 都作为投影处理，设计一致
3. **消除漏报**：覆盖之前保守策略的盲区
4. **仍避免误报**：静态变量场景仍然正确处理
5. **实现简单**：不需要复杂的指针别名分析

### 限制 ⚠️
1. **不跟踪别名**：`let p2 = p1; drop(*p1); *p2` 可能漏报
   - 原因：`*p1` 和 `*p2` 是不同的 ID，即使它们指向同一内容
   - 解决：需要引入指针别名分析（类似 lockbud）

2. **不跟踪内容逃逸**：
   ```rust
   let ptr = vec.as_mut_ptr();  // 内容"逃逸"到 ptr
   drop(vec);                    // vec 被 drop
   *ptr;                         // ❌ 但工具可能检测不到
   ```
   - 原因：没有跟踪 `as_mut_ptr` 返回的指针指向 vec 的内容
   - 解决：需要跨函数的逃逸分析

3. **字段敏感性有限**：
   ```rust
   drop((*ptr).field1);  // drop 一个字段
   (*ptr).field2;        // 访问另一个字段，可能误报
   ```
   - 原因：当前的 drop 可能标记整个 `*ptr`，而不仅是 `*ptr.field1`
   - 解决：需要更细粒度的字段敏感跟踪

## 与保守策略的对比

| 方面 | 保守策略（之前） | 精确跟踪（现在） |
|------|-----------------|----------------|
| **false positive** | ✅ 低（避免误报） | ✅ 低（仍然避免） |
| **false negative** | ❌ 高（漏报 heap UAF） | ✅ 低（能检测大部分） |
| **实现复杂度** | ✅ 简单（跳过 Deref） | ⚠️ 中等（扩展 ID 系统） |
| **静态变量** | ✅ 正确处理 | ✅ 正确处理 |
| **堆指针** | ❌ 无法检测 | ✅ 能检测 |
| **指针别名** | ❌ 无法检测 | ⚠️ 部分检测（通过依赖） |

## 未来改进方向

### 短期
1. **增强测试**：添加更多边缘情况的测试
2. **性能优化**：ID 字符串操作的优化（目前影响很小）
3. **错误信息**：更友好的错误消息，区分不同类型的解引用错误

### 中期
1. **别名分析**：检测 `p1` 和 `p2` 指向同一内容的情况
2. **逃逸分析**：跟踪 `as_ptr()`、`as_mut_ptr()` 等函数返回的指针
3. **字段敏感性**：区分结构体不同字段的 drop 状态

### 长期
1. **堆内存跟踪**：为每个堆分配创建抽象位置
2. **路径敏感性改进**：结合 k-predecessor DFS 进一步提升精度
3. **跨函数分析**：建立函数摘要，处理跨函数的指针传递

## 使用建议

1. **启用调试输出**：
   ```bash
   DEBUG_MIR=1 cargo taint-ana
   ```
   可以看到每个 ID 的详细跟踪信息

2. **理解新的 ID 格式**：
   - `*_21` 表示一次解引用
   - `_21.0@deref` 表示字段后解引用
   - 报错信息中会显示完整 ID

3. **测试你的代码**：
   使用 `src/toys/deref_tracking_test.rs` 作为参考，创建你自己的测试用例

## 总结

本次实现通过扩展 ID 系统，**将解引用作为独立的 ID 进行跟踪**，在保持低误报率的同时，**显著降低了漏报率**。这是一个在**实现复杂度和检测能力之间良好平衡的方案**，为未来更高级的分析（如别名分析、逃逸分析）奠定了基础。

---

**实现日期**：2026-01-07  
**实现者**：AI Assistant  
**相关文件**：
- `src/detect.rs` - 核心实现
- `src/toys/deref_tracking_test.rs` - 测试用例
- `.cursor/plans/支持解引用id跟踪_9d081609.plan.md` - 实现计划

