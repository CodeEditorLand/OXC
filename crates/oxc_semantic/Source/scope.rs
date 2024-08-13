use std::hash::BuildHasherDefault;

use indexmap::IndexMap;
use oxc_index::IndexVec;
use oxc_span::CompactStr;
use oxc_syntax::reference::{ReferenceFlag, ReferenceId};
pub use oxc_syntax::scope::{ScopeFlags, ScopeId};
use rustc_hash::{FxHashMap, FxHasher};

use crate::{symbol::SymbolId, AstNodeId};

type FxIndexMap<K, V> = IndexMap<K, V, BuildHasherDefault<FxHasher>>;

pub(crate) type Bindings = FxIndexMap<CompactStr, SymbolId>;
pub(crate) type UnresolvedReference = (ReferenceId, ReferenceFlag);
pub type UnresolvedReferences = FxHashMap<CompactStr, Vec<UnresolvedReference>>;

/// Scope Tree
///
/// The scope tree stores lexical scopes created by a program, and all the
/// variable bindings each scope creates.
///
/// - All scopes have a parent scope, except the root scope.
/// - Scopes can have 0 or more child scopes.
/// - Nodes that create a scope store the [`ScopeId`] of the scope they create.
///
/// `SoA` (Struct of Arrays) for memory efficiency.
#[derive(Debug, Default)]
pub struct ScopeTree {
    /// Maps a scope to the parent scope it belongs in.
    parent_ids: IndexVec<ScopeId, Option<ScopeId>>,
    /// Maps a scope to direct children scopes.
    child_ids: IndexVec<ScopeId, Vec<ScopeId>>,
    /// Maps a scope to its node id.
    node_ids: IndexVec<ScopeId, AstNodeId>,
    flags: IndexVec<ScopeId, ScopeFlags>,
    /// Symbol bindings in a scope.
    ///
    /// A binding is a mapping from an identifier name to its [`SymbolId`]
    bindings: IndexVec<ScopeId, Bindings>,
    pub(crate) root_unresolved_references: UnresolvedReferences,
}

impl ScopeTree {
    const ROOT_SCOPE_ID: ScopeId = ScopeId::new(0);

    /// Returns the number of scopes found in the program. Includes the root
    /// program scope.
    #[inline]
    pub fn len(&self) -> usize {
        self.parent_ids.len()
    }

    /// Returns `true` if there are no scopes in the program.
    ///
    /// This will always return `false` when semantic analysis has completed
    /// since there is a root scope.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.parent_ids.is_empty()
    }

    /// Iterate over the scopes that contain a scope.
    ///
    /// The first element of this iterator will be the scope itself. This
    /// guarantees the iterator will have at least 1 element.
    pub fn ancestors(&self, scope_id: ScopeId) -> impl Iterator<Item = ScopeId> + '_ {
        std::iter::successors(Some(scope_id), |scope_id| self.parent_ids[*scope_id])
    }

    /// Iterate over scopes contained by a scope in breadth-first order.
    ///
    /// Unlike [`ancestors`], this iterator will not include the scope itself.
    ///
    /// [`ancestors`]: ScopeTree::ancestors
    pub fn descendants(&self, scope_id: ScopeId) -> impl Iterator<Item = ScopeId> + '_ {
        // Has to be a `fn` and pass arguments because we can't
        // have recursive closures
        fn add_to_list(
            parent_id: ScopeId,
            child_ids: &IndexVec<ScopeId, Vec<ScopeId>>,
            items: &mut Vec<ScopeId>,
        ) {
            if let Some(children) = child_ids.get(parent_id) {
                for child_id in children {
                    items.push(*child_id);
                    add_to_list(*child_id, child_ids, items);
                }
            }
        }

        let mut list = vec![];

        add_to_list(scope_id, &self.child_ids, &mut list);

        list.into_iter()
    }

    /// Get the child scopes of a scope.
    ///
    /// Will return [`None`] if no scope exists, which should never happen if
    /// you obtained `scope_id` through valid means. Scopes with no children
    /// return [`Some`] empty [`Vec`].
    #[inline]
    pub fn get_child_ids(&self, scope_id: ScopeId) -> Option<&Vec<ScopeId>> {
        self.child_ids.get(scope_id)
    }

    /// Get a mutable reference to a scope's children.
    ///
    /// Will return [`None`] if no scope exists, which should never happen if
    /// you obtained `scope_id` through valid means. Scopes with no children
    /// return [`Some`] empty [`Vec`].
    #[inline]
    pub fn get_child_ids_mut(&mut self, scope_id: ScopeId) -> Option<&mut Vec<ScopeId>> {
        self.child_ids.get_mut(scope_id)
    }

    pub fn descendants_from_root(&self) -> impl Iterator<Item = ScopeId> + '_ {
        self.parent_ids.iter_enumerated().map(|(scope_id, _)| scope_id)
    }

    /// Get the root [`Program`] scope id.
    ///
    /// [`Program`]: oxc_ast::ast::Program
    #[inline]
    pub const fn root_scope_id(&self) -> ScopeId {
        Self::ROOT_SCOPE_ID
    }

    /// Get the flags for the root scope.
    ///
    /// This is a shorthand for `scope.get_flags(scope.root_scope_id())`.
    #[inline]
    pub fn root_flags(&self) -> ScopeFlags {
        self.flags[self.root_scope_id()]
    }

    #[inline]
    pub fn root_unresolved_references(&self) -> &UnresolvedReferences {
        &self.root_unresolved_references
    }

    pub fn root_unresolved_references_ids(
        &self,
    ) -> impl Iterator<Item = impl Iterator<Item = ReferenceId> + '_> + '_ {
        self.root_unresolved_references.values().map(|v| v.iter().map(|(id, _)| *id))
    }

    #[inline]
    pub fn get_flags(&self, scope_id: ScopeId) -> ScopeFlags {
        self.flags[scope_id]
    }

    #[inline]
    pub fn get_flags_mut(&mut self, scope_id: ScopeId) -> &mut ScopeFlags {
        &mut self.flags[scope_id]
    }

    /// Get [`ScopeFlags`] for a new child scope under `parent_scope_id`.
    pub fn get_new_scope_flags(
        &self,
        mut flags: ScopeFlags,
        parent_scope_id: ScopeId,
    ) -> ScopeFlags {
        // https://tc39.es/ecma262/#sec-strict-mode-code
        let parent_scope_flags = self.get_flags(parent_scope_id);
        flags |= parent_scope_flags & ScopeFlags::StrictMode;

        // inherit flags for non-function scopes
        if !flags.contains(ScopeFlags::Function) {
            flags |= parent_scope_flags & ScopeFlags::Modifiers;
        }

        flags
    }

    #[inline]
    pub fn get_parent_id(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.parent_ids[scope_id]
    }

    pub fn set_parent_id(&mut self, scope_id: ScopeId, parent_id: Option<ScopeId>) {
        self.parent_ids[scope_id] = parent_id;
        if let Some(parent_id) = parent_id {
            self.child_ids[parent_id].push(scope_id);
        }
    }

    /// Get a variable binding by name that was declared in the top-level scope
    #[inline]
    pub fn get_root_binding(&self, name: &str) -> Option<SymbolId> {
        self.get_binding(self.root_scope_id(), name)
    }

    pub fn add_root_unresolved_reference(
        &mut self,
        name: CompactStr,
        reference: UnresolvedReference,
    ) {
        self.root_unresolved_references.entry(name).or_default().push(reference);
    }

    /// Check if a symbol is declared in a certain scope.
    pub fn has_binding(&self, scope_id: ScopeId, name: &str) -> bool {
        self.bindings[scope_id].get(name).is_some()
    }

    /// Get the symbol bound to an identifier name in a scope.
    ///
    /// Returns [`None`] if that name is not bound in the scope. This could be
    /// because the symbol is not declared within this tree, but it could also
    /// be because its declaration is in a parent scope. If you want to find a
    /// binding that might be declared in a parent scope, use [`find_binding`].
    ///
    /// [`find_binding`]: ScopeTree::find_binding
    pub fn get_binding(&self, scope_id: ScopeId, name: &str) -> Option<SymbolId> {
        self.bindings[scope_id].get(name).copied()
    }

    /// Find a binding by name in a scope or its ancestors.
    ///
    /// Bindings are resolved by walking up the scope tree until a binding is
    /// found. If no binding is found, [`None`] is returned.
    pub fn find_binding(&self, scope_id: ScopeId, name: &str) -> Option<SymbolId> {
        for scope_id in self.ancestors(scope_id) {
            if let Some(symbol_id) = self.bindings[scope_id].get(name) {
                return Some(*symbol_id);
            }
        }
        None
    }

    /// Get all bound identifiers in a scope.
    #[inline]
    pub fn get_bindings(&self, scope_id: ScopeId) -> &Bindings {
        &self.bindings[scope_id]
    }

    /// Get the ID of the [`AstNode`] that created a scope.
    ///
    /// [`AstNode`]: crate::AstNode
    #[inline]
    pub fn get_node_id(&self, scope_id: ScopeId) -> AstNodeId {
        self.node_ids[scope_id]
    }

    /// Iterate over all bindings declared in the entire program.
    ///
    /// If you only want bindings in a specific scope, use [`iter_bindings_in`].
    ///
    /// [`iter_bindings_in`]: ScopeTree::iter_bindings_in
    pub fn iter_bindings(&self) -> impl Iterator<Item = (ScopeId, SymbolId, &'_ CompactStr)> + '_ {
        self.bindings.iter_enumerated().flat_map(|(scope_id, bindings)| {
            bindings.iter().map(move |(name, symbol_id)| (scope_id, *symbol_id, name))
        })
    }

    /// Iterate over bindings declared inside a scope.
    #[inline]
    pub fn iter_bindings_in(&self, scope_id: ScopeId) -> impl Iterator<Item = SymbolId> + '_ {
        self.bindings[scope_id].values().copied()
    }

    #[inline]
    pub(crate) fn get_bindings_mut(&mut self, scope_id: ScopeId) -> &mut Bindings {
        &mut self.bindings[scope_id]
    }

    /// Create a scope inside another scope.
    ///
    /// For the root [`Program`] scope, use [`add_root_scope`].
    ///
    /// [`Program`]: oxc_ast::ast::Program
    /// [`add_root_scope`]: ScopeTree::add_root_scope
    pub fn add_scope(
        &mut self,
        parent_id: ScopeId,
        node_id: AstNodeId,
        flags: ScopeFlags,
    ) -> ScopeId {
        let scope_id = self.add_scope_impl(Some(parent_id), node_id, flags);

        // Set this scope as child of parent scope
        self.child_ids[parent_id].push(scope_id);

        scope_id
    }

    /// Create the root [`Program`] scope.
    ///
    /// Do not use this method if a root scope already exists. Use [`add_scope`]
    /// to create a new scope inside the root scope.
    ///
    /// [`Program`]: oxc_ast::ast::Program
    /// [`add_scope`]: ScopeTree::add_scope
    pub fn add_root_scope(&mut self, node_id: AstNodeId, flags: ScopeFlags) -> ScopeId {
        self.add_scope_impl(None, node_id, flags)
    }

    // `#[inline]` because almost always called from `add_scope` and want to avoid
    // overhead of a function call there.
    #[inline]
    fn add_scope_impl(
        &mut self,
        parent_id: Option<ScopeId>,
        node_id: AstNodeId,
        flags: ScopeFlags,
    ) -> ScopeId {
        let scope_id = self.parent_ids.push(parent_id);
        self.child_ids.push(vec![]);
        self.flags.push(flags);
        self.bindings.push(Bindings::default());
        self.node_ids.push(node_id);
        scope_id
    }

    /// Add a binding to a scope.
    ///
    /// [`binding`]: Bindings
    pub fn add_binding(&mut self, scope_id: ScopeId, name: CompactStr, symbol_id: SymbolId) {
        self.bindings[scope_id].insert(name, symbol_id);
    }

    /// Remove an existing binding from a scope.
    pub fn remove_binding(&mut self, scope_id: ScopeId, name: &CompactStr) {
        self.bindings[scope_id].shift_remove(name);
    }

    /// Reserve memory for an `additional` number of scopes.
    pub fn reserve(&mut self, additional: usize) {
        self.parent_ids.reserve(additional);
        self.child_ids.reserve(additional);
        self.flags.reserve(additional);
        self.bindings.reserve(additional);
        self.node_ids.reserve(additional);
    }
}