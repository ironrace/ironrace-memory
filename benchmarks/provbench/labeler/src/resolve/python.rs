//! Python `SymbolResolver` — tree-sitter scope walker + lexical import graph.
//!
//! # Indexing pass (per repo at HEAD)
//!
//! Walk every `.py` file under `repo_root` in sorted order (`std::fs::read_dir`
//! results are explicitly sorted at every level — determinism is load-bearing).
//! For each file we parse with [`crate::ast::python::PythonAst`] and record
//! three kinds of state:
//!
//! - Module-level bindings (`function_definition`, `class_definition`, and
//!   `expression_statement > assignment` with a single-identifier LHS),
//!   keyed by `(module_path, name)`.
//! - Class members. For each `class_definition` we walk its body for direct
//!   member definitions (methods + class attributes), keyed by
//!   `(module_path, class_name, member_name)`. Nested classes and method
//!   bodies are not descended for member purposes — same convention as
//!   `facts::python::field`.
//! - Import edges. `import …` / `from … import …` are recorded in
//!   `import_graph`, keyed by `(importing_module_path, local_alias)` →
//!   `(target_module, target_symbol)`. `from X import *` is skipped (see
//!   limitations).
//!
//! Resolution follows edges at query time (single-hop only — see
//! `lookup_in_module`).
//!
//! # Resolution pass (per `resolve(qualified_name)` call)
//!
//! 1. Split the qualified name on `.`.
//! 2. Try the longest prefix that names a module known to the index. The
//!    remaining suffix must match either a top-level binding (length 1),
//!    a `class.member` pair (length 2), or an import alias that we can
//!    follow ONCE into a different module.
//! 3. Return `Some(ResolvedLocation { file, line })` if found, else `None`.
//!
//! # Known limitations
//!
//! Documented as hygiene flags for the v1.2b findings doc:
//!
//! - `from X import *` — skipped entirely; no `__all__` interpretation.
//! - Relative imports (`from .foo import bar`) — treated as unresolvable;
//!   the `module_name` child is a `relative_import` node, not a
//!   `dotted_name`, and we punt rather than guess the package root.
//! - `__init__.py` re-exports — NOT collapsed. A file at `pkg/__init__.py`
//!   is indexed as the module `pkg.__init__`. Real flask uses sparse
//!   `__init__` re-exports, which this resolver will under-resolve;
//!   flagged for the v1.2b findings doc.
//! - Dynamic dispatch, metaclass attribute generation, conditional
//!   `TYPE_CHECKING` imports — not modeled.
//! - Multi-hop import chains — we follow at most one edge per resolution
//!   to avoid the need for cycle detection on the static graph.

use anyhow::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ast::python::PythonAst;
use crate::resolve::{ResolvedLocation, SymbolResolver};
use tree_sitter::Node;

/// In-memory Python symbol index. Cheap to construct; `index` walks the
/// repo once and resolution is then `BTreeMap` lookups.
pub struct PythonResolver {
    repo_root: PathBuf,
    /// `(module_path, top_level_name)` → def location.
    module_bindings: BTreeMap<(String, String), ResolvedLocation>,
    /// `(module_path, class_name, member_name)` → def location.
    class_bindings: BTreeMap<(String, String, String), ResolvedLocation>,
    /// `(importing_module, local_alias)` → `(target_module, target_symbol)`.
    /// `target_symbol` is the empty string for plain `import X` edges.
    import_graph: BTreeMap<(String, String), (String, String)>,
}

impl PythonResolver {
    /// Index every `.py` file under `repo_root` in sorted order.
    pub fn index(repo_root: impl Into<PathBuf>) -> Result<Self> {
        let repo_root = repo_root.into();
        let mut r = Self {
            repo_root: repo_root.clone(),
            module_bindings: BTreeMap::new(),
            class_bindings: BTreeMap::new(),
            import_graph: BTreeMap::new(),
        };
        let py_files = collect_python_files(&repo_root)?;
        for file in py_files {
            r.index_file(&file)?;
        }
        Ok(r)
    }

    fn module_path_for(&self, file: &Path) -> Option<String> {
        let rel = file.strip_prefix(&self.repo_root).ok()?;
        // Use a slash-joined logical path regardless of host separator so
        // resolutions are byte-identical on Linux + macOS. (Windows is not
        // a target for the labeler — but using components avoids surprises
        // if it ever becomes one.)
        let mut parts: Vec<String> = rel
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        let last = parts.last_mut()?;
        if let Some(stripped) = last.strip_suffix(".py") {
            *last = stripped.to_string();
        }
        Some(parts.join("."))
    }

    fn index_file(&mut self, file: &Path) -> Result<()> {
        let src = std::fs::read(file)?;
        let ast = PythonAst::parse(&src)?;
        let module_path = match self.module_path_for(file) {
            Some(p) => p,
            None => return Ok(()), // file lives outside repo_root; skip
        };

        let root = ast.root();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            match child.kind() {
                "function_definition" => {
                    self.record_module_def(child, &src, &module_path, file);
                }
                "class_definition" => {
                    self.record_class_and_members(child, &src, &module_path, file);
                }
                "decorated_definition" => {
                    // `@decorator def foo(): ...` / `@decorator class Foo: ...`
                    if let Some(def) = decorated_inner(child) {
                        match def.kind() {
                            "function_definition" => {
                                self.record_module_def(def, &src, &module_path, file);
                            }
                            "class_definition" => {
                                self.record_class_and_members(def, &src, &module_path, file);
                            }
                            _ => {}
                        }
                    }
                }
                "expression_statement" => {
                    self.record_module_assignment(child, &src, &module_path, file);
                }
                "import_statement" => {
                    self.record_import(child, &src, &module_path);
                }
                "import_from_statement" => {
                    self.record_import_from(child, &src, &module_path);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn record_module_def(&mut self, node: Node<'_>, src: &[u8], module_path: &str, file: &Path) {
        if let Some((name, line)) = def_name_and_line(node, src) {
            self.module_bindings.insert(
                (module_path.to_string(), name),
                ResolvedLocation {
                    file: file.to_path_buf(),
                    line,
                },
            );
        }
    }

    fn record_class_and_members(
        &mut self,
        class_node: Node<'_>,
        src: &[u8],
        module_path: &str,
        file: &Path,
    ) {
        let (class_name, class_line) = match def_name_and_line(class_node, src) {
            Some(v) => v,
            None => return,
        };
        self.module_bindings.insert(
            (module_path.to_string(), class_name.clone()),
            ResolvedLocation {
                file: file.to_path_buf(),
                line: class_line,
            },
        );

        let body = match class_node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        };
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "function_definition" => {
                    if let Some((member, line)) = def_name_and_line(child, src) {
                        self.class_bindings.insert(
                            (module_path.to_string(), class_name.clone(), member),
                            ResolvedLocation {
                                file: file.to_path_buf(),
                                line,
                            },
                        );
                    }
                }
                "decorated_definition" => {
                    if let Some(def) = decorated_inner(child) {
                        if def.kind() == "function_definition" {
                            if let Some((member, line)) = def_name_and_line(def, src) {
                                self.class_bindings.insert(
                                    (module_path.to_string(), class_name.clone(), member),
                                    ResolvedLocation {
                                        file: file.to_path_buf(),
                                        line,
                                    },
                                );
                            }
                        }
                    }
                }
                "expression_statement" => {
                    if let Some((field_name, line)) = single_ident_assignment(child, src) {
                        self.class_bindings.insert(
                            (module_path.to_string(), class_name.clone(), field_name),
                            ResolvedLocation {
                                file: file.to_path_buf(),
                                line,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn record_module_assignment(
        &mut self,
        stmt: Node<'_>,
        src: &[u8],
        module_path: &str,
        file: &Path,
    ) {
        if let Some((name, line)) = single_ident_assignment(stmt, src) {
            self.module_bindings.insert(
                (module_path.to_string(), name),
                ResolvedLocation {
                    file: file.to_path_buf(),
                    line,
                },
            );
        }
    }

    /// `import X`, `import X.Y`, `import X as Z`, multi-name forms.
    fn record_import(&mut self, node: Node<'_>, src: &[u8], importing_module: &str) {
        // `import_statement` children: one or more `dotted_name` or
        // `aliased_import` nodes (separated by commas, which are anonymous).
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "dotted_name" => {
                    let full = dotted_name_text(child, src);
                    if full.is_empty() {
                        continue;
                    }
                    // `import X.Y.Z` binds the leftmost segment `X` locally
                    // and the value of that name is the package `X` itself.
                    // We record an edge from `X` → `(X, "")` so a query for
                    // `<mod>.X.something` falls through to a module lookup
                    // on `X.something` at resolution time.
                    let alias = full.split('.').next().unwrap_or("").to_string();
                    if alias.is_empty() {
                        continue;
                    }
                    self.import_graph.insert(
                        (importing_module.to_string(), alias),
                        (
                            full.split('.').next().unwrap_or("").to_string(),
                            String::new(),
                        ),
                    );
                }
                "aliased_import" => {
                    if let Some((target_module, alias)) = aliased_import_parts(child, src) {
                        // `import X.Y as Z` → alias `Z` resolves to the full
                        // dotted target `X.Y`.
                        self.import_graph.insert(
                            (importing_module.to_string(), alias),
                            (target_module, String::new()),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// `from X import Y`, `from X import Y as Z`, `from X import *` (skipped).
    fn record_import_from(&mut self, node: Node<'_>, src: &[u8], importing_module: &str) {
        let module_node = match node.child_by_field_name("module_name") {
            Some(m) => m,
            None => return,
        };
        // Relative imports (`from .foo import bar`) have a `relative_import`
        // module child rather than a `dotted_name`. We punt — see module doc.
        if module_node.kind() != "dotted_name" {
            return;
        }
        let target_module = dotted_name_text(module_node, src);
        if target_module.is_empty() {
            return;
        }

        // The imported names live as `name:` field children — there may be
        // multiple. Tree-sitter-python emits a `wildcard_import` node for
        // `from X import *`; we leave it untouched.
        let mut cursor = node.walk();
        for child in node.children_by_field_name("name", &mut cursor) {
            match child.kind() {
                "dotted_name" => {
                    let name = dotted_name_text(child, src);
                    if name.is_empty() {
                        continue;
                    }
                    // Local alias == imported symbol name.
                    self.import_graph.insert(
                        (importing_module.to_string(), name.clone()),
                        (target_module.clone(), name),
                    );
                }
                "aliased_import" => {
                    if let Some((symbol, alias)) = aliased_import_parts(child, src) {
                        self.import_graph.insert(
                            (importing_module.to_string(), alias),
                            (target_module.clone(), symbol),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// Walk `segs` against the index, starting from the module `module`.
    /// `segs` is the suffix that follows the module portion of the original
    /// qualified name (so for `src.example.Greeter.greet` with `module ==
    /// "src.example"`, `segs == ["Greeter", "greet"]`).
    ///
    /// `import_hops_remaining` bounds the number of times we may follow an
    /// import edge — we cap at one to avoid the need for cycle detection.
    fn lookup_in_module(
        &self,
        module: &str,
        segs: &[&str],
        import_hops_remaining: u32,
    ) -> Option<ResolvedLocation> {
        if segs.is_empty() {
            return None;
        }
        let head = segs[0];
        match segs.len() {
            1 => {
                if let Some(loc) = self
                    .module_bindings
                    .get(&(module.to_string(), head.to_string()))
                {
                    return Some(loc.clone());
                }
            }
            2 => {
                // class.member
                if let Some(loc) = self.class_bindings.get(&(
                    module.to_string(),
                    head.to_string(),
                    segs[1].to_string(),
                )) {
                    return Some(loc.clone());
                }
            }
            _ => {}
        }

        // Try following an import edge from this module via `head`.
        if import_hops_remaining > 0 {
            if let Some((target_module, target_symbol)) = self
                .import_graph
                .get(&(module.to_string(), head.to_string()))
            {
                let mut next_segs: Vec<&str> = Vec::new();
                if !target_symbol.is_empty() {
                    next_segs.push(target_symbol.as_str());
                }
                next_segs.extend_from_slice(&segs[1..]);
                if !next_segs.is_empty() {
                    return self.lookup_in_module(target_module, &next_segs, 0);
                }
            }
        }
        None
    }
}

impl SymbolResolver for PythonResolver {
    fn resolve(&mut self, qualified_name: &str) -> Result<Option<ResolvedLocation>> {
        let segs: Vec<&str> = qualified_name.split('.').collect();
        if segs.is_empty() {
            return Ok(None);
        }

        // Longest-prefix module match. We require at least one trailing
        // segment so the suffix can identify a binding.
        for prefix_end in (1..segs.len()).rev() {
            let module = segs[..prefix_end].join(".");
            // Cheap pre-filter: is any module binding registered for this
            // module path? If not, skip to the next prefix.
            //
            // We use `range` on the BTreeMap to find the first key whose
            // module matches without scanning the whole map.
            let start = (module.clone(), String::new());
            let has_module = self
                .module_bindings
                .range(start..)
                .next()
                .map(|((m, _), _)| m == &module)
                .unwrap_or(false);
            if !has_module {
                continue;
            }
            let remaining = &segs[prefix_end..];
            if let Some(loc) = self.lookup_in_module(&module, remaining, 1) {
                return Ok(Some(loc));
            }
        }
        Ok(None)
    }
}

// ── tree-sitter helpers ──────────────────────────────────────────────────────

fn def_name_and_line(node: Node<'_>, src: &[u8]) -> Option<(String, u32)> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(src).ok()?.to_string();
    let line = (node.start_position().row + 1) as u32;
    Some((name, line))
}

/// `expression_statement > assignment` whose `left` is a single identifier.
/// Returns `(name, line)` or `None`. Mirrors the convention used by
/// `facts::python::field` and `facts::python::symbol_existence`.
fn single_ident_assignment(stmt: Node<'_>, src: &[u8]) -> Option<(String, u32)> {
    let mut cursor = stmt.walk();
    let assignment = stmt
        .named_children(&mut cursor)
        .find(|c| c.kind() == "assignment")?;
    let left = assignment.child_by_field_name("left")?;
    if left.kind() != "identifier" {
        return None;
    }
    let name = left.utf8_text(src).ok()?.to_string();
    let line = (stmt.start_position().row + 1) as u32;
    Some((name, line))
}

/// Decorated definitions wrap a `function_definition` or `class_definition`.
/// Returns the inner def node.
fn decorated_inner(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let result = node
        .named_children(&mut cursor)
        .find(|c| matches!(c.kind(), "function_definition" | "class_definition"));
    result
}

/// Concatenate the identifier children of a `dotted_name` with `.`.
fn dotted_name_text(node: Node<'_>, src: &[u8]) -> String {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|c| c.kind() == "identifier")
        .filter_map(|c| c.utf8_text(src).ok().map(|s| s.to_string()))
        .collect::<Vec<_>>()
        .join(".")
}

/// `aliased_import` has `name: dotted_name` and `alias: identifier` fields.
/// Returns `(full_dotted_name, alias)`.
fn aliased_import_parts(node: Node<'_>, src: &[u8]) -> Option<(String, String)> {
    let name_node = node.child_by_field_name("name")?;
    let alias_node = node.child_by_field_name("alias")?;
    let name = dotted_name_text(name_node, src);
    if name.is_empty() {
        return None;
    }
    let alias = alias_node.utf8_text(src).ok()?.to_string();
    Some((name, alias))
}

// ── filesystem walk ──────────────────────────────────────────────────────────

/// Sorted recursive walk of `repo_root` returning every `*.py` file.
/// Directory entries are sorted by file name at every level so the index
/// order is byte-stable across runs.
fn collect_python_files(repo_root: &Path) -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = Vec::new();
    walk_dir(repo_root, &mut out)?;
    Ok(out)
}

fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for path in entries {
        // Skip common noise directories. Determinism still holds because
        // the sort happens before the filter.
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if matches!(
                    name,
                    ".git" | "__pycache__" | ".venv" | "venv" | "node_modules"
                ) {
                    continue;
                }
            }
            walk_dir(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("py") {
            out.push(path);
        }
    }
    Ok(())
}
