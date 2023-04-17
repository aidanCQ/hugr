//! The Hugr data structure.
//!
//! TODO: metadata
#![allow(dead_code)]

use portgraph::{Hierarchy, NodeIndex, PortGraph, SecondaryMap};
use thiserror::Error;

use crate::ops::{ModuleOp, OpType};
use crate::rewrite::{Rewrite, RewriteError};

pub mod serialize;

/// The Hugr data structure.
#[derive(Clone, Debug, PartialEq)]
pub struct Hugr {
    /// The graph encoding the adjacency structure of the HUGR.
    pub(crate) graph: PortGraph,

    /// The node hierarchy.
    hierarchy: Hierarchy,

    /// The single root node in the hierarchy.
    /// It must correspond to a [`ModuleOp::Root`] node.
    ///
    /// [`ModuleOp::Root`]: crate::ops::ModuleOp::Root
    root: NodeIndex,

    /// Operation types for each node.
    op_types: SecondaryMap<NodeIndex, OpType>,
}

impl Default for Hugr {
    fn default() -> Self {
        Self::new()
    }
}

impl Hugr {
    /// Create a new Hugr, with a single root node.
    pub(crate) fn new() -> Self {
        let mut graph = PortGraph::default();
        let hierarchy = Hierarchy::new();
        let mut op_types = SecondaryMap::new();
        let root = graph.add_node(0, 0);
        op_types[root] = OpType::Module(ModuleOp::Root);

        Self {
            graph,
            hierarchy,
            root,
            op_types,
        }
    }

    /// Add a node to the graph.
    pub fn add_node(&mut self, op: OpType) -> NodeIndex {
        let sig = op.signature();
        let node = self.graph.add_node(sig.input.len(), sig.output.len());
        self.op_types[node] = op;
        node
    }

    /// Connect two nodes at the given ports.
    pub fn connect(
        &mut self,
        src: NodeIndex,
        src_port: usize,
        dst: NodeIndex,
        dst_port: usize,
    ) -> Result<(), HugrError> {
        self.graph.link_nodes(src, src_port, dst, dst_port)?;
        Ok(())
    }

    /// Sets the parent of a node.
    ///
    /// The node becomes the parent's last child.
    pub fn set_parent(&mut self, node: NodeIndex, parent: NodeIndex) -> Result<(), HugrError> {
        self.hierarchy.push_child(node, parent)?;
        Ok(())
    }

    /// Applies a rewrite to the graph.
    pub fn apply_rewrite(mut self, rewrite: Rewrite) -> Result<(), RewriteError> {
        // Get the open graph for the rewrites, and a HUGR with the additional components.
        let (rewrite, mut replacement, parents) = rewrite.into_parts();

        // TODO: Use `parents` to update the hierarchy, and keep the internal hierarchy from `replacement`.
        let _ = parents;

        let node_inserted = |old, new| {
            std::mem::swap(&mut self.op_types[new], &mut replacement.op_types[old]);
            // TODO: metadata (Fn parameter ?)
        };
        rewrite.apply_with_callbacks(
            &mut self.graph,
            |_| {},
            |_| {},
            node_inserted,
            |_, _| {},
            |_, _| {},
        )?;

        // TODO: Check types

        Ok(())
    }

    /// Check the validity of the HUGR.
    pub fn validate(&self) -> Result<(), ValidationError> {
        // TODO
        Ok(())
    }

    pub fn root(&self) -> NodeIndex {
        self.root
    }
}

/// Errors that can occur while manipulating a Hugr.
///
/// TODO: Better descriptions, not just re-exporting portgraph errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum HugrError {
    /// An error occurred while connecting nodes.
    #[error("An error occurred while connecting the nodes.")]
    ConnectionError(#[from] portgraph::LinkError),
    /// An error occurred while manipulating the hierarchy.
    #[error("An error occurred while manipulating the hierarchy.")]
    HierarchyError(#[from] portgraph::hierarchy::AttachError),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValidationError {}
