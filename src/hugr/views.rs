//! Read-only access into HUGR graphs and subgraphs.

pub mod descendants;
pub mod petgraph;
pub mod sibling;
pub mod sibling_subgraph;

#[cfg(test)]
mod tests;

pub use self::petgraph::PetgraphWrapper;
pub use descendants::DescendantsGraph;
use ouroboros::self_referencing;
pub use sibling::SiblingGraph;
pub use sibling_subgraph::SiblingSubgraph;

use context_iterators::{ContextIterator, IntoContextIterator, MapWithCtx};
use itertools::{Itertools, MapInto};
use portgraph::dot::{DotFormat, EdgeStyle, NodeStyle, PortStyle};
use portgraph::{multiportgraph, LinkView, MultiPortGraph, PortView};

use super::{Hugr, HugrError, NodeMetadata, NodeType, DEFAULT_NODETYPE};
use crate::ops::handle::NodeHandle;
use crate::ops::{FuncDecl, FuncDefn, OpName, OpTag, OpType, DFG};
use crate::types::{EdgeKind, FunctionType};
use crate::{Direction, Node, Port};

/// A trait for inspecting HUGRs.
/// For end users we intend this to be superseded by region-specific APIs.
pub trait HugrView<'m>: sealed::HugrInternals<'m> {
    /// The kind of handle that can be used to refer to the root node.
    ///
    /// The handle is guaranteed to be able to contain the operation returned by
    /// [`HugrView::root_type`].
    type RootHandle: NodeHandle;

    /// An Iterator over the nodes in a Hugr(View)
    type Nodes<'a>: Iterator<Item = Node>
    where
        Self: 'a;

    /// An Iterator over the nodes in a Hugr(View) - used where the iterator must outlive
    /// the HugrView, so some data will be copied into it
    type Nodes2<'a>: Iterator<Item = Node> where 'm: 'a;

    /// An Iterator over (some or all) ports of a node
    type NodePorts<'a>: Iterator<Item = Port>
    where
        Self: 'a;

    /// An Iterator over the children of a node
    type Children<'a>: Iterator<Item = Node>
    where
        Self: 'a;

    /// An Iterator over (some or all) the nodes neighbouring a node
    type Neighbours<'a>: Iterator<Item = Node>
    where
        Self: 'a;

    /// Iterator over the children of a node
    type PortLinks<'a>: Iterator<Item = (Node, Port)>
    where
        Self: 'a;

    /// Iterator over the links between two nodes.
    type NodeConnections<'a>: Iterator<Item = [Port; 2]>
    where
        Self: 'a;

    /// Return the root node of this view.
    #[inline]
    fn root(&self) -> Node {
        self.root_node()
    }

    /// Return the type of the HUGR root node.
    #[inline]
    fn root_type<'a>(&'a self) -> &NodeType where 'm: 'a {
        let node_type = self.get_nodetype(self.root());
        debug_assert!(Self::RootHandle::can_hold(node_type.tag()));
        node_type
    }

    /// Returns whether the node exists.
    fn contains_node(&self, node: Node) -> bool;

    /// Validates that a node is valid in the graph.
    ///
    /// Returns a [`HugrError::InvalidNode`] otherwise.
    #[inline]
    fn valid_node(&self, node: Node) -> Result<(), HugrError> {
        match self.contains_node(node) {
            true => Ok(()),
            false => Err(HugrError::InvalidNode(node)),
        }
    }

    /// Validates that a node is a valid root descendant in the graph.
    ///
    /// To include the root node use [`HugrView::valid_node`] instead.
    ///
    /// Returns a [`HugrError::InvalidNode`] otherwise.
    #[inline]
    fn valid_non_root(&self, node: Node) -> Result<(), HugrError> {
        match self.root() == node {
            true => Err(HugrError::InvalidNode(node)),
            false => self.valid_node(node),
        }
    }

    /// Returns the parent of a node.
    #[inline]
    fn get_parent(&self, node: Node) -> Option<Node> {
        self.valid_non_root(node).ok()?;
        self.base_hugr()
            .hierarchy
            .parent(node.index)
            .map(Into::into)
    }

    /// Returns the operation type of a node.
    #[inline]
    fn get_optype<'a>(&'a self, node: Node) -> &OpType where 'm: 'a {
        &self.get_nodetype(node).op
    }

    /// Returns the type of a node.
    #[inline]
    fn get_nodetype<'a>(&'a self, node: Node) -> &NodeType where 'm: 'a {
        match self.contains_node(node) {
            true => self.base_hugr().op_types.get(node.index),
            false => &DEFAULT_NODETYPE,
        }
    }

    /// Returns the metadata associated with a node.
    #[inline]
    fn get_metadata<'a>(&'a self, node: Node) -> &NodeMetadata where 'm: 'a {
        match self.contains_node(node) {
            true => self.base_hugr().metadata.get(node.index),
            false => &NodeMetadata::Null,
        }
    }

    /// Returns the number of nodes in the hugr.
    fn node_count(&self) -> usize;

    /// Returns the number of edges in the hugr.
    fn edge_count(&self) -> usize;

    /// Iterates over the nodes in the port graph.
    fn nodes(&self) -> Self::Nodes<'_>;

    fn nodes2(&self) -> Self::Nodes2<'m>;

    /// Iterator over ports of node in a given direction.
    fn node_ports(&self, node: Node, dir: Direction) -> Self::NodePorts<'_>;

    /// Iterator over output ports of node.
    /// Shorthand for [`node_ports`][HugrView::node_ports]`(node, Direction::Outgoing)`.
    #[inline]
    fn node_outputs(&self, node: Node) -> Self::NodePorts<'_> {
        self.node_ports(node, Direction::Outgoing)
    }

    /// Iterator over inputs ports of node.
    /// Shorthand for [`node_ports`][HugrView::node_ports]`(node, Direction::Incoming)`.
    #[inline]
    fn node_inputs(&self, node: Node) -> Self::NodePorts<'_> {
        self.node_ports(node, Direction::Incoming)
    }

    /// Iterator over both the input and output ports of node.
    fn all_node_ports(&self, node: Node) -> Self::NodePorts<'_>;

    /// Iterator over the nodes and ports connected to a port.
    fn linked_ports(&self, node: Node, port: Port) -> Self::PortLinks<'_>;

    /// Iterator the links between two nodes.
    fn node_connections(&self, node: Node, other: Node) -> Self::NodeConnections<'_>;

    /// Returns whether a port is connected.
    fn is_linked(&self, node: Node, port: Port) -> bool {
        self.linked_ports(node, port).next().is_some()
    }

    /// Number of ports in node for a given direction.
    fn num_ports(&self, node: Node, dir: Direction) -> usize;

    /// Number of inputs to a node.
    /// Shorthand for [`num_ports`][HugrView::num_ports]`(node, Direction::Incoming)`.
    #[inline]
    fn num_inputs(&self, node: Node) -> usize {
        self.num_ports(node, Direction::Incoming)
    }

    /// Number of outputs from a node.
    /// Shorthand for [`num_ports`][HugrView::num_ports]`(node, Direction::Outgoing)`.
    #[inline]
    fn num_outputs(&self, node: Node) -> usize {
        self.num_ports(node, Direction::Outgoing)
    }

    /// Return iterator over children of node.
    fn children(&self, node: Node) -> Self::Children<'_>;

    /// Iterates over neighbour nodes in the given direction.
    /// May contain duplicates if the graph has multiple links between nodes.
    fn neighbours(&self, node: Node, dir: Direction) -> Self::Neighbours<'_>;

    /// Iterates over the input neighbours of the `node`.
    /// Shorthand for [`neighbours`][HugrView::neighbours]`(node, Direction::Incoming)`.
    #[inline]
    fn input_neighbours(&self, node: Node) -> Self::Neighbours<'_> {
        self.neighbours(node, Direction::Incoming)
    }

    /// Iterates over the output neighbours of the `node`.
    /// Shorthand for [`neighbours`][HugrView::neighbours]`(node, Direction::Outgoing)`.
    #[inline]
    fn output_neighbours(&self, node: Node) -> Self::Neighbours<'_> {
        self.neighbours(node, Direction::Outgoing)
    }

    /// Iterates over the input and output neighbours of the `node` in sequence.
    fn all_neighbours(&self, node: Node) -> Self::Neighbours<'_>;

    /// Get the input and output child nodes of a dataflow parent.
    /// If the node isn't a dataflow parent, then return None
    fn get_io(&self, node: Node) -> Option<[Node; 2]>;

    /// For function-like HUGRs (DFG, FuncDefn, FuncDecl), report the function
    /// type. Otherwise return None.
    fn get_function_type<'a>(&'a self) -> Option<&FunctionType> where 'm: 'a {
        let op = self.get_nodetype(self.root());
        match &op.op {
            OpType::DFG(DFG { signature })
            | OpType::FuncDecl(FuncDecl { signature, .. })
            | OpType::FuncDefn(FuncDefn { signature, .. }) => Some(signature),
            _ => None,
        }
    }

    /// Return a wrapper over the view that can be used in petgraph algorithms.
    #[inline]
    fn as_petgraph(&self) -> PetgraphWrapper<'_, Self>
    where
        Self: Sized,
    {
        PetgraphWrapper { hugr: self }
    }

    /// Return dot string showing underlying graph and hierarchy side by side.
    fn dot_string(&self) -> String {
        let hugr = self.base_hugr();
        let graph = self.portgraph();
        graph
            .dot_format()
            .with_hierarchy(&hugr.hierarchy)
            .with_node_style(|n| {
                NodeStyle::Box(format!(
                    "({ni}) {name}",
                    ni = n.index(),
                    name = self.get_optype(n.into()).name()
                ))
            })
            .with_port_style(|port| {
                let node = graph.port_node(port).unwrap();
                let optype = self.get_optype(node.into());
                let offset = graph.port_offset(port).unwrap();
                match optype.port_kind(offset).unwrap() {
                    EdgeKind::Static(ty) => {
                        PortStyle::new(html_escape::encode_text(&format!("{}", ty)))
                    }
                    EdgeKind::Value(ty) => {
                        PortStyle::new(html_escape::encode_text(&format!("{}", ty)))
                    }
                    EdgeKind::StateOrder => match graph.port_links(port).count() > 0 {
                        true => PortStyle::text("", false),
                        false => PortStyle::Hidden,
                    },
                    _ => PortStyle::text("", true),
                }
            })
            .with_edge_style(|src, tgt| {
                let src_node = graph.port_node(src).unwrap();
                let src_optype = self.get_optype(src_node.into());
                let src_offset = graph.port_offset(src).unwrap();
                let tgt_node = graph.port_node(tgt).unwrap();

                if hugr.hierarchy.parent(src_node) != hugr.hierarchy.parent(tgt_node) {
                    EdgeStyle::Dashed
                } else if src_optype.port_kind(src_offset) == Some(EdgeKind::StateOrder) {
                    EdgeStyle::Dotted
                } else {
                    EdgeStyle::Solid
                }
            })
            .finish()
    }
}

/// A common trait for views of a HUGR hierarchical subgraph.
pub trait HierarchyView<'a>: HugrView<'a> + Sized {
    /// Create a hierarchical view of a HUGR given a root node.
    ///
    /// # Errors
    /// Returns [`HugrError::InvalidNode`] if the root isn't a node of the required [OpTag]
    fn try_new(hugr: &'a impl HugrView<'a>, root: Node) -> Result<Self, HugrError>; // ALAN ?
}

#[self_referencing]
struct Holder<S, T> {
    s: S,
    #[borrows(s)]
    t: T
}

impl<S, T:Iterator> Iterator for Holder<S,T> {
    type Item = T::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.with_t_mut(|t| t.next())
    }
}

impl<T> HugrView<'static> for T
where
    T: AsRef<Hugr> + 'static,
{
    type RootHandle = Node;

    /// An Iterator over the nodes in a Hugr(View)
    type Nodes<'a> = MapInto<multiportgraph::Nodes<'a>, Node> where Self: 'a;

    type Nodes2<'a> = Holder<MultiPortGraph, MapInto<multiportgraph::Nodes<'a>, Node>>;

    /// An Iterator over (some or all) ports of a node
    type NodePorts<'a> = MapInto<portgraph::portgraph::NodePortOffsets, Port> where Self: 'a;

    /// An Iterator over the children of a node
    type Children<'a> = MapInto<portgraph::hierarchy::Children<'a>, Node> where Self: 'a;

    /// An Iterator over (some or all) the nodes neighbouring a node
    type Neighbours<'a> = MapInto<multiportgraph::Neighbours<'a>, Node> where Self: 'a;

    /// Iterator over the children of a node
    type PortLinks<'a> = MapWithCtx<multiportgraph::PortLinks<'a>, &'a Hugr, (Node, Port)>
    where
        Self: 'a;

    type NodeConnections<'a> = MapWithCtx<multiportgraph::NodeConnections<'a>,&'a Hugr, [Port; 2]> where Self: 'a;

    #[inline]
    fn contains_node(&self, node: Node) -> bool {
        self.as_ref().graph.contains_node(node.index)
    }

    #[inline]
    fn node_count(&self) -> usize {
        self.as_ref().graph.node_count()
    }

    #[inline]
    fn edge_count(&self) -> usize {
        self.as_ref().graph.link_count()
    }

    #[inline]
    fn nodes(&self) -> Self::Nodes<'_> {
        self.as_ref().graph.nodes_iter().map_into()
    }

    fn nodes2(&self) -> Self::Nodes2<'static> {
        Holder::new(self.as_ref().graph.clone(),
       |g| g.nodes_iter().map_into())
    }

    #[inline]
    fn node_ports(&self, node: Node, dir: Direction) -> Self::NodePorts<'_> {
        self.as_ref().graph.port_offsets(node.index, dir).map_into()
    }

    #[inline]
    fn all_node_ports(&self, node: Node) -> Self::NodePorts<'_> {
        self.as_ref().graph.all_port_offsets(node.index).map_into()
    }

    #[inline]
    fn linked_ports(&self, node: Node, port: Port) -> Self::PortLinks<'_> {
        let hugr = self.as_ref();
        let port = hugr.graph.port_index(node.index, port.offset).unwrap();
        hugr.graph
            .port_links(port)
            .with_context(hugr)
            .map_with_context(|(_, link), hugr| {
                let port = link.port();
                let node = hugr.graph.port_node(port).unwrap();
                let offset = hugr.graph.port_offset(port).unwrap();
                (node.into(), offset.into())
            })
    }

    #[inline]
    fn node_connections(&self, node: Node, other: Node) -> Self::NodeConnections<'_> {
        let hugr = self.as_ref();

        hugr.graph
            .get_connections(node.index, other.index)
            .with_context(hugr)
            .map_with_context(|(p1, p2), hugr| {
                [p1, p2].map(|link| {
                    let offset = hugr.graph.port_offset(link.port()).unwrap();
                    offset.into()
                })
            })
    }

    #[inline]
    fn num_ports(&self, node: Node, dir: Direction) -> usize {
        self.as_ref().graph.num_ports(node.index, dir)
    }

    #[inline]
    fn children(&self, node: Node) -> Self::Children<'_> {
        self.as_ref().hierarchy.children(node.index).map_into()
    }

    #[inline]
    fn neighbours(&self, node: Node, dir: Direction) -> Self::Neighbours<'_> {
        self.as_ref().graph.neighbours(node.index, dir).map_into()
    }

    #[inline]
    fn all_neighbours(&self, node: Node) -> Self::Neighbours<'_> {
        self.as_ref().graph.all_neighbours(node.index).map_into()
    }

    #[inline]
    fn get_io(&self, node: Node) -> Option<[Node; 2]> {
        let op = self.get_nodetype(node);
        if OpTag::DataflowParent.is_superset(op.tag()) {
            self.children(node).take(2).collect_vec().try_into().ok()
        } else {
            None
        }
    }
}

pub(crate) mod sealed {
    use super::*;

    /// Trait for accessing the internals of a Hugr(View).
    ///
    /// Specifically, this trait provides access to the underlying portgraph
    /// view.
    pub trait HugrInternals<'g> {
        /// The underlying portgraph view type.
        type Portgraph<'p>: LinkView + Clone + 'p
        where
            Self: 'p;

        /// Returns a reference to the underlying portgraph.
        fn portgraph(&self) -> Self::Portgraph<'_>;

        /// Returns the Hugr at the base of a chain of views.
        fn base_hugr(&self) -> &'g Hugr;

        /// Return the root node of this view.
        fn root_node(&self) -> Node;
    }

    impl<'g, T> HugrInternals<'g> for T
    where
        T: AsRef<super::Hugr> + 'g,
    {
        type Portgraph<'p> = &'p MultiPortGraph where Self: 'p;

        #[inline]
        fn portgraph(&self) -> Self::Portgraph<'_> {
            &self.as_ref().graph
        }

        #[inline]
        fn base_hugr(&self) -> &'g Hugr {
            self.as_ref()
        }

        #[inline]
        fn root_node(&self) -> Node {
            self.as_ref().root.into()
        }
    }
}
