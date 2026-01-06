# LockBud 架构深度解析

## 概述

LockBud 是一个基于 Rust MIR 的静态分析工具，主要用于检测并发和内存安全问题。本文档详细分析其架构设计、核心算法和实现细节。

---

## 🏗️ 整体架构

### 三层设计模式

```
┌─────────────────────────────────────────┐
│         Detector Layer                  │
│  (UseAfterFreeDetector, DeadlockDetector)│
└─────────────┬───────────────────────────┘
              │ 查询别名关系
┌─────────────▼───────────────────────────┐
│      AliasAnalysis Layer                │
│  (Andersen 指针分析 + 跨函数启发式)      │
└─────────────┬───────────────────────────┘
              │ 查询调用关系
┌─────────────▼───────────────────────────┐
│       CallGraph Layer                   │
│  (全局调用图 + 闭包追踪)                 │
└─────────────────────────────────────────┘
```

**设计理念**：
- **自底向上构建** - 先构建调用图，再进行指针分析，最后执行检测
- **按需分析** - 指针分析结果缓存，避免重复计算
- **分离关注点** - 每层负责不同的抽象级别

---

## 📊 Layer 1: CallGraph（调用图）

### 核心数据结构

```rust
pub struct CallGraph<'tcx> {
    // 使用 petgraph 的有向图
    pub graph: Graph<CallGraphNode<'tcx>, Vec<CallSiteLocation>, Directed>,
}

pub enum CallGraphNode<'tcx> {
    WithBody(Instance<'tcx>),     // 有 MIR body 的实例
    WithoutBody(Instance<'tcx>),  // 外部函数/内部函数
}

pub enum CallSiteLocation {
    Direct(Location),          // 直接调用
    ClosureDef(Local),        // 闭包定义位置
}
```

**设计要点**：
1. **节点** = 单态化实例（Instance），不是函数定义（DefId）
2. **边** = 调用点列表，支持多个调用点
3. **特殊处理闭包** - 记录闭包定义位置和捕获变量

### 构建流程

```rust
pub fn analyze(
    &mut self,
    instances: Vec<Instance<'tcx>>,  // 所有可实例化的函数
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
) {
    // 1. 添加所有节点
    for inst in instances {
        let idx = self.graph.add_node(CallGraphNode::WithBody(inst));
        // ...
    }
    
    // 2. 遍历每个函数的 MIR，收集调用点
    for (caller_idx, caller) in idx_insts {
        let body = tcx.instance_mir(caller.def);
        let mut collector = CallSiteCollector::new(caller, body, tcx, typing_env);
        collector.visit_body(body);
        
        // 3. 为每个调用点添加边
        for (callee, location) in collector.finish() {
            let callee_idx = /* 查找或创建 callee 节点 */;
            self.graph.add_edge(caller_idx, callee_idx, vec![location]);
        }
    }
}
```

### 关键特性

#### 1. 单态化感知

```rust
// 不同的泛型实例被视为不同的节点
Vec::<i32>::new()  // Instance 1
Vec::<String>::new()  // Instance 2
```

**优势**：
- 类型信息精确
- 避免泛型导致的误报

#### 2. 闭包定义追踪

```rust
fn visit_local_decl(&mut self, local: Local, local_decl: &LocalDecl<'tcx>) {
    if let TyKind::Closure(def_id, substs) = func_ty.kind() {
        // 记录闭包实例和定义位置
        self.callsites.push((callee_instance, CallSiteLocation::ClosureDef(local)));
    }
}
```

**作用**：
- 追踪闭包的捕获变量（upvars）
- 支持跨函数的闭包分析

#### 3. 路径查询

```rust
// 查找从 source 到 target 的所有简单路径
pub fn all_simple_paths(&self, source: InstanceId, target: InstanceId) 
    -> Vec<Vec<InstanceId>>
```

**用途**：
- 检测潜在的调用链
- 分析跨函数的数据流

---

## 🔍 Layer 2: Andersen 指针分析

### 核心思想

**Andersen 算法** 是一种基于约束的指针分析：
1. 从 MIR 收集指针赋值约束
2. 通过固定点迭代传播指针关系
3. 得到 `points-to` 集合

### 约束类型

```rust
enum ConstraintEdge {
    Address,    // a = &b       → pts(a) ∋ b
    Copy,       // a = b        → pts(a) ⊇ pts(b)
    Load,       // a = *b       → ∀o∈pts(b), pts(a) ⊇ pts(o)
    Store,      // *a = b       → ∀o∈pts(a), pts(o) ⊇ pts(b)
    AliasCopy,  // a = Arc::clone(b) → 特殊处理
}
```

**核心不变式**：
```
如果 x 指向 y，则 pts(x) 包含 y
```

### 约束节点设计

```rust
pub enum ConstraintNode<'tcx> {
    Alloc(PlaceRef<'tcx>),          // 分配节点
    Place(PlaceRef<'tcx>),          // 内存位置
    Constant(Const<'tcx>),          // 常量/静态变量
    ConstantDeref(Const<'tcx>),     // *常量
}
```

**关键设计：静态变量处理**

```rust
fn add_constant(&mut self, constant: Const<'tcx>) {
    let lhs = ConstraintNode::Constant(constant);
    let rhs = ConstraintNode::ConstantDeref(constant);
    
    // Constant(C) --|address|--> ConstantDeref(C)
    self.graph.add_edge(rhs, lhs, ConstraintEdge::Address);
    
    // ConstantDeref(C) --|address|--> ConstantDeref(C)
    // 处理多级解引用：*C, **C, ***C 都指向 *C
    self.graph.add_edge(rhs, rhs, ConstraintEdge::Address);
}
```

**为什么这样设计？**

| 场景 | 节点 | 含义 |
|-----|------|------|
| `STATIC` | `Constant(STATIC)` | 静态变量本身 |
| `*STATIC` | `ConstantDeref(STATIC)` | 静态变量的内容 |
| `**STATIC` | `ConstantDeref(STATIC)` | 自引用，避免无限递归 |

### 固定点算法

```rust
pub fn analyze(&mut self) {
    let mut worklist = VecDeque::new();
    
    // 1. 初始化：为每个 Place 添加 Alloc 约束
    for node in graph.nodes() {
        match node {
            ConstraintNode::Place(place) => {
                graph.add_alloc(place);  // place = alloc
            }
        }
        worklist.push_back(node);
    }
    
    // 2. 处理 Address 约束
    for (source, target, weight) in graph.edges() {
        if weight == ConstraintEdge::Address {
            self.pts.entry(target).or_default().insert(source);
            worklist.push_back(target);
        }
    }
    
    // 3. 固定点迭代
    while let Some(node) = worklist.pop_front() {
        for o in self.pts.get(&node).unwrap() {
            // Store: *node = source
            for source in graph.store_sources(&node) {
                if graph.insert_edge(source, o, ConstraintEdge::Copy) {
                    worklist.push_back(source);
                }
            }
            
            // Load: target = *node
            for target in graph.load_targets(&node) {
                if graph.insert_edge(o, target, ConstraintEdge::Copy) {
                    worklist.push_back(o);
                }
            }
        }
        
        // Copy: target = node
        for target in graph.copy_targets(&node) {
            if self.union_pts(&target, &node) {
                worklist.push_back(target);
            }
        }
    }
}
```

**时间复杂度**：O(n³) 最坏情况，实际上通常更好

### 字段敏感性

```rust
// 支持嵌套字段
Place { local: _1, projection: [Field(0), Field(1)] }
// 表示 _1.0.1
```

**投影处理**：
```rust
fn process_place(place_ref: PlaceRef<'tcx>) -> AccessPattern<'tcx> {
    match place_ref {
        PlaceRef { local, projection: [ProjectionElem::Deref, ..] } => {
            // (*x).field → 间接访问
            AccessPattern::Indirect(...)
        }
        _ => AccessPattern::Direct(place_ref)
    }
}
```

### 特殊函数处理

```rust
// Arc::clone / Rc::clone
if ownership::is_arc_or_rc_clone(def_id, substs, tcx) {
    // dest --|alias_copy|--> arg
    // dest --|load|--> arg
    self.graph.add_alias_copy(dest, arg);
    self.graph.add_load(dest, arg);
}

// Vec::as_mut_ptr
if name.contains("as_mut_ptr") {
    // dest --|copy|--> arg (指针别名)
    self.graph.add_copy(dest, arg);
}
```

---

## 🌐 跨函数别名分析

### 挑战

**问题**：函数内指针分析无法知道不同函数的变量是否别名

**示例**：
```rust
fn foo(x: &mut Vec<i32>) {
    let ptr = x.as_mut_ptr();  // ptr in foo
}

fn bar(y: &mut Vec<i32>) {
    let ptr = y.as_mut_ptr();  // ptr in bar
}

// ptr in foo 和 ptr in bar 是否别名？
```

### LockBud 的启发式方案

#### 1. 相同常量别名

```rust
fn point_to_same_constant<'tcx>(
    pts1: &FxHashSet<ConstraintNode<'tcx>>,
    pts2: &FxHashSet<ConstraintNode<'tcx>>,
) -> bool {
    // 检查两个指针是否都指向同一个常量
    let constants1 = pts1.iter().filter(|n| matches!(n, ConstraintNode::ConstantDeref(_)));
    let constants2 = pts2.iter().filter(|n| matches!(n, ConstraintNode::ConstantDeref(_)));
    constants1.any(|c1| constants2.any(|c2| c2 == c1))
}
```

**场景**：
```rust
static GLOBAL: Mutex<i32> = Mutex::new(0);

fn func1() { let x = &GLOBAL; }
fn func2() { let y = &GLOBAL; }
// x 和 y 指向同一个常量 → Probably 别名
```

#### 2. 相同类型参数别名

```rust
fn point_to_same_type_param<'tcx>(
    pts1: &FxHashSet<ConstraintNode<'tcx>>,
    pts2: &FxHashSet<ConstraintNode<'tcx>>,
    body1: &Body<'tcx>,
    body2: &Body<'tcx>,
) -> bool {
    // 如果两个指针都指向同类型的函数参数 → 可能别名
    let params1 = pts1.iter().filter_map(|node| {
        if is_parameter(node.local, body1) {
            Some((node.ty(body1), node.projection))
        } else { None }
    });
    
    let params2 = pts2.iter().filter_map(|node| {
        if is_parameter(node.local, body2) {
            Some((node.ty(body2), node.projection))
        } else { None }
    });
    
    params1.any(|p1| params2.any(|p2| p1.ty == p2.ty && p1.projection == p2.projection))
}
```

**场景**：
```rust
fn func1(x: &mut Vec<i32>) { let p1 = x.as_mut_ptr(); }
fn func2(y: &mut Vec<i32>) { let p2 = y.as_mut_ptr(); }
// p1 和 p2 都指向同类型参数 → Possibly 别名
```

#### 3. 闭包捕获变量别名

```rust
fn interproc_alias(...) -> Option<ApproximateAliasKind> {
    // 如果 p1 在闭包中
    if self.tcx.is_closure_like(instance1.def_id()) {
        // 回溯到定义闭包的函数
        let defsite_upvars = self.closure_defsite_upvars(instance1, ...);
        
        // 检查 p2 是否指向闭包的捕获变量
        for (def_inst, upvar) in defsite_upvars {
            if def_inst.def_id() == instance2.def_id() {
                let alias_kind = self.intraproc_points_to(def_inst, node2, upvar);
                if alias_kind > ApproximateAliasKind::Unlikely {
                    return Some(alias_kind);
                }
            }
        }
    }
}
```

**场景**：
```rust
fn outer() {
    let mut v = vec![1, 2, 3];
    let ptr1 = v.as_mut_ptr();
    
    let closure = || {
        let ptr2 = v.as_mut_ptr();  // 捕获 v
    };
}
// ptr1 和 ptr2 都指向 v → Possibly 别名
```

### 别名等级

```rust
pub enum ApproximateAliasKind {
    Probably,   // 几乎确定别名（同一常量、同一 local）
    Possibly,   // 可能别名（同类型参数、闭包捕获）
    Unlikely,   // 不太可能别名
    Unknown,    // 无法判断
}
```

**偏序关系**：Probably > Possibly > Unlikely > Unknown

---

## 🐛 Layer 3: Use-After-Free 检测

### 三种检测模式

#### 模式 1：逃逸到全局变量

```rust
fn collect_raw_ptrs_escape_to_global<'tcx>(
    pts: &PointsToMap<'tcx>,
    body: &Body<'tcx>,
    tcx: TyCtxt<'tcx>,
) -> FxHashSet<(ConstraintNode<'tcx>, ConstraintNode<'tcx>)> {
    pts.iter()
        .filter_map(|(ptr, ptes)| {
            // 找到所有 ConstantDeref（全局变量）
            if let ConstraintNode::ConstantDeref(_) = ptr {
                Some((ptr, ptes))
            } else { None }
        })
        .flat_map(|(ptr, ptes)| {
            // 找到指向局部变量的原始指针
            ptes.iter()
                .filter_map(|pte| match pte {
                    ConstraintNode::Alloc(place) if place.ty(body, tcx).is_raw_ptr() => {
                        Some((ConstraintNode::Place(*place), ptr.clone()))
                    }
                    _ => None,
                })
        })
        .collect()
}
```

**检测逻辑**：
1. 找到所有存储在全局变量中的原始指针
2. 检查这些指针指向的内存是否被 drop
3. 如果 drop 了，报告 bug

**示例**：
```rust
static mut GLOBAL_PTR: *mut Vec<i32> = ptr::null_mut();

fn bug() {
    let v = vec![1, 2, 3];
    unsafe { GLOBAL_PTR = v.as_mut_ptr(); }
    drop(v);  // ❌ v 被 drop，但 GLOBAL_PTR 仍指向它
}
```

#### 模式 2：逃逸到返回值/参数

```rust
fn detect_escape_to_return_or_param<'tcx>(...) -> FxHashSet<String> {
    for (ptr, ptes) in pts {
        let ptr = match ptr {
            ConstraintNode::Place(ptr) => ptr,
            _ => continue,
        };
        
        // 找到别名于参数/返回值的指针
        let mut alias_with_params = Vec::new();
        let mut alias_with_raw_ptrs = Vec::new();
        
        for pte in ptes {
            match pte {
                ConstraintNode::Alloc(pte) => {
                    if pte.local < first_non_param_local {
                        // 指向参数
                        alias_with_params.push(pte);
                    } else if pte.ty(body, tcx).is_raw_ptr() {
                        // 指向原始指针
                        alias_with_raw_ptrs.push(pte);
                    }
                }
            }
        }
        
        // 检查原始指针指向的内存是否被 drop
        for raw_ptr in alias_with_raw_ptrs {
            let ptes = pts.get(&ConstraintNode::Place(*raw_ptr))?;
            for pte in ptes {
                for (location, drop_place) in drops {
                    if drop_place.as_ref() == pte {
                        // 报告：指针通过参数/返回值逃逸，但指向已 drop 的内存
                    }
                }
            }
        }
    }
}
```

**示例**：
```rust
fn bug(out: &mut *mut Vec<i32>) {
    let v = vec![1, 2, 3];
    *out = v.as_mut_ptr();  // 逃逸到参数
    drop(v);  // ❌ v 被 drop
}
```

#### 模式 3：函数内 use-after-drop

```rust
fn detect_use_after_drop<'tcx>(
    raw_ptrs: &FxHashSet<Local>,
    pts: &PointsToMap<'tcx>,
    drops: &[(Location, Place<'tcx>)],
    body: &Body<'tcx>,
) -> FxHashSet<String> {
    for raw_ptr in raw_ptrs {
        let ptes = pts.get(&ConstraintNode::Place(Place::from(*raw_ptr)))?;
        let raw_ptr_use_locations = find_uses(body, *raw_ptr);
        
        for pte in ptes {
            for (drop_loc, drop_place) in drops {
                if drop_place.as_ref() == pte {
                    // 检查 drop 后是否使用
                    for use_loc in &raw_ptr_use_locations {
                        if is_reachable(*drop_loc, *use_loc, body) {
                            // 报告：use-after-drop
                        }
                    }
                }
            }
        }
    }
}
```

**关键**：使用控制流可达性分析 `is_reachable`

**示例**：
```rust
fn bug() {
    let mut v = vec![1, 2, 3];
    let ptr = v.as_mut_ptr();
    drop(v);  // drop 在 bb1
    unsafe { println!("{}", *ptr); }  // use 在 bb2，bb1 → bb2 可达
}
```

### Drop 收集

#### 自动 Drop

```rust
struct AutoDropCollector<'tcx> {
    drop_locations: Vec<(Location, Place<'tcx>)>,
}

impl Visitor<'tcx> for AutoDropCollector<'tcx> {
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        if let TerminatorKind::Drop { place, .. } = &terminator.kind {
            self.drop_locations.push((location, *place));
        }
    }
}
```

#### 手动 Drop

```rust
fn collect_manual_drop<'tcx>(
    callgraph: &CallGraph<'tcx>,
    tcx: TyCtxt<'tcx>,
) -> FxHashMap<InstanceId, Vec<(Location, Place<'tcx>)>> {
    let mut manual_drops = FxHashMap::default();
    
    // 1. 在调用图中找到 std::mem::drop
    for (callee_id, node) in callgraph.graph.node_references() {
        let path = tcx.def_path_str(instance.def_id());
        if !path.starts_with("std::mem::drop") { continue; }
        
        // 2. 找到所有调用 drop 的地方
        for caller_id in callgraph.callers(callee_id) {
            let callsites = callgraph.callsites(caller_id, callee_id)?;
            for loc in callsites {
                let arg = /* 提取第一个参数 */;
                manual_drops.entry(caller_id).or_default().push((loc, arg));
            }
        }
    }
    
    manual_drops
}
```

---

## 💡 设计亮点与局限

### 亮点

#### 1. 模块化设计
- 每层独立，易于测试和维护
- 可以单独使用 CallGraph 或 AliasAnalysis

#### 2. 缓存优化
```rust
pub struct AliasAnalysis<'a, 'tcx> {
    pts: FxHashMap<DefId, PointsToMap<'tcx>>,  // 缓存指针分析结果
}

pub fn get_or_insert_pts(&mut self, def_id: DefId, body: &Body<'tcx>) {
    if self.pts.contains_key(&def_id) {
        return self.pts.get(&def_id).unwrap();
    }
    // 执行指针分析并缓存
}
```

#### 3. 保守但实用的跨函数分析
- 不需要完整的过程间数据流
- 使用类型信息作为启发式
- 在精度和性能之间取得平衡

#### 4. 特殊处理 Rust 特性
- 闭包捕获变量
- Arc/Rc 引用计数
- 智能指针

### 局限

#### 1. 流不敏感
```rust
// lockbud 无法区分以下两种情况
let ptr = v.as_mut_ptr();
drop(v);
// ... 无法知道 drop 是否在 use 之前
```

**解决**：使用控制流可达性弥补

#### 2. 上下文不敏感
```rust
fn callee(x: &mut Vec<i32>) { /* ... */ }

fn caller1() { let v1 = vec![1]; callee(&mut v1); }
fn caller2() { let v2 = vec![2]; callee(&mut v2); }
// lockbud 将 v1 和 v2 混在一起分析
```

**影响**：可能产生误报

#### 3. 字段不敏感（结构体级别）
```rust
struct S { a: Vec<i32>, b: Vec<i32> }
// lockbud 只支持字段访问，但不精确区分不同字段的别名
```

---

## 📊 性能特征

### 时间复杂度

| 阶段 | 复杂度 | 说明 |
|-----|--------|------|
| CallGraph 构建 | O(n) | n = 函数数量 |
| 指针分析 | O(n³) | 最坏情况，实际通常 O(n²) |
| 别名查询 | O(1) | 缓存查表 |
| 检测 | O(m) | m = 原始指针数量 |

### 空间复杂度

| 数据结构 | 复杂度 | 说明 |
|---------|--------|------|
| CallGraph | O(n + e) | n=节点，e=边 |
| PointsToMap | O(n × p) | p=平均 points-to 集大小 |
| 缓存 | O(f × s) | f=函数数，s=单函数状态大小 |

---

## 🎯 与我们的工具对比

| 特性 | LockBud | 我们的工具 |
|-----|---------|-----------|
| **分析范围** | 全局（跨函数） | 函数内 |
| **别名分析** | Andersen（约束求解） | Union-Find |
| **静态变量** | Constant 节点 | 类型检查 |
| **路径敏感** | 否（流不敏感） | 是（k-predecessor DFS） |
| **性能** | 较慢（全局分析） | 快速（函数内） |
| **精度** | 中等（保守启发式） | 高（路径敏感） |
| **实现复杂度** | 高（3层架构） | 中等 |

### 我们可以借鉴的

1. ✅ **Constant 节点设计** - 明确区分静态变量
2. ✅ **类型驱动启发式** - 用类型信息辅助分析
3. ✅ **缓存机制** - 避免重复计算
4. ⚠️ **跨函数分析** - 需要更复杂的架构

### 我们的优势

1. ✅ **路径敏感** - 更精确的控制流分析
2. ✅ **轻量级** - 更快的分析速度
3. ✅ **易于扩展** - 简单的架构

---

## 📚 参考资料

### 论文
- Andersen, L. O. (1994). "Program Analysis and Specialization for the C Programming Language"
- LockBud 论文（如果有的话）

### 相关工具
- **MIRAI** - Facebook 的 Rust 静态分析工具
- **Prusti** - ETH Zurich 的 Rust 验证工具
- **Rudra** - Rust 内存安全检测器

### Rust 相关
- Rust MIR 文档
- rustc_middle API 文档

---

## 💭 总结

LockBud 展示了如何构建一个**工业级的静态分析工具**：

1. **分层架构** - 清晰的关注点分离
2. **理论基础** - 基于经典的 Andersen 算法
3. **工程权衡** - 在精度、性能和实用性之间平衡
4. **Rust 特化** - 充分利用 Rust 的类型系统

对于我们的工具，**不需要完全复制 LockBud**，而是：
- 理解其设计思路
- 借鉴其核心技术（如 Constant 节点）
- 保持我们的优势（路径敏感、轻量级）
- 针对性地解决特定问题（如静态变量误报）

**最终目标**：构建一个**简单、快速、精确**的内存安全检测工具！

