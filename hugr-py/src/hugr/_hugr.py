from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass, field, replace
from enum import Enum
from typing import (
    ClassVar,
    Generic,
    Iterable,
    Iterator,
    Protocol,
    TypeVar,
    cast,
    overload,
    Type as PyType,
)

from typing_extensions import Self

from hugr._ops import Op, DataflowOp
from hugr._tys import Type, Kind
from hugr.serialization.ops import OpType as SerialOp
from hugr.serialization.serial_hugr import SerialHugr
from hugr.utils import BiMap

from ._exceptions import ParentBeforeChild


class Direction(Enum):
    INCOMING = 0
    OUTGOING = 1


@dataclass(frozen=True, eq=True, order=True)
class _Port:
    node: Node
    offset: int
    direction: ClassVar[Direction]


@dataclass(frozen=True, eq=True, order=True)
class InPort(_Port):
    direction: ClassVar[Direction] = Direction.INCOMING


class Wire(Protocol):
    def out_port(self) -> OutPort: ...


@dataclass(frozen=True, eq=True, order=True)
class OutPort(_Port, Wire):
    direction: ClassVar[Direction] = Direction.OUTGOING

    def out_port(self) -> OutPort:
        return self


class ToNode(Wire, Protocol):
    def to_node(self) -> Node: ...

    @overload
    def __getitem__(self, index: int) -> OutPort: ...
    @overload
    def __getitem__(self, index: slice) -> Iterator[OutPort]: ...
    @overload
    def __getitem__(self, index: tuple[int, ...]) -> Iterator[OutPort]: ...

    def __getitem__(
        self, index: int | slice | tuple[int, ...]
    ) -> OutPort | Iterator[OutPort]:
        return self.to_node()._index(index)

    def out_port(self) -> "OutPort":
        return OutPort(self.to_node(), 0)

    def inp(self, offset: int) -> InPort:
        return InPort(self.to_node(), offset)

    def out(self, offset: int) -> OutPort:
        return OutPort(self.to_node(), offset)

    def port(self, offset: int, direction: Direction) -> InPort | OutPort:
        if direction == Direction.INCOMING:
            return self.inp(offset)
        else:
            return self.out(offset)


@dataclass(frozen=True, eq=True, order=True)
class Node(ToNode):
    idx: int
    _num_out_ports: int | None = field(default=None, compare=False)

    def _index(
        self, index: int | slice | tuple[int, ...]
    ) -> OutPort | Iterator[OutPort]:
        match index:
            case int(index):
                if self._num_out_ports is not None:
                    if index >= self._num_out_ports:
                        raise IndexError("Index out of range")
                return self.out(index)
            case slice():
                start = index.start or 0
                stop = index.stop or self._num_out_ports
                if stop is None:
                    raise ValueError(
                        "Stop must be specified when number of outputs unknown"
                    )
                step = index.step or 1
                return (self[i] for i in range(start, stop, step))
            case tuple(xs):
                return (self[i] for i in xs)

    def to_node(self) -> Node:
        return self


@dataclass()
class NodeData:
    op: Op
    parent: Node | None
    _num_inps: int = 0
    _num_outs: int = 0
    children: list[Node] = field(default_factory=list)

    def to_serial(self, node: Node, hugr: Hugr) -> SerialOp:
        o = self.op.to_serial(node, self.parent if self.parent else node, hugr)

        return SerialOp(root=o)  # type: ignore[arg-type]


P = TypeVar("P", InPort, OutPort)
K = TypeVar("K", InPort, OutPort)
OpVar = TypeVar("OpVar", bound=Op)
OpVar2 = TypeVar("OpVar2", bound=Op)


@dataclass(frozen=True, eq=True, order=True)
class _SubPort(Generic[P]):
    port: P
    sub_offset: int = 0

    def next_sub_offset(self) -> Self:
        return replace(self, sub_offset=self.sub_offset + 1)


_SO = _SubPort[OutPort]
_SI = _SubPort[InPort]


class ParentBuilder(ToNode, Protocol[OpVar]):
    hugr: Hugr[OpVar]
    parent_node: Node

    def to_node(self) -> Node:
        return self.parent_node

    @property
    def parent_op(self) -> OpVar:
        return cast(OpVar, self.hugr[self.parent_node].op)


@dataclass()
class Hugr(Mapping[Node, NodeData], Generic[OpVar]):
    root: Node
    _nodes: list[NodeData | None]
    _links: BiMap[_SO, _SI]
    _free_nodes: list[Node]

    def __init__(self, root_op: OpVar) -> None:
        self._free_nodes = []
        self._links = BiMap()
        self._nodes = []
        self.root = self._add_node(root_op, None, 0)

    def __getitem__(self, key: ToNode) -> NodeData:
        key = key.to_node()
        try:
            n = self._nodes[key.idx]
        except IndexError:
            n = None
        if n is None:
            raise KeyError(key)
        return n

    def __iter__(self):
        return iter(self._nodes)

    def __len__(self) -> int:
        return self.num_nodes()

    def _get_typed_op(self, node: ToNode, cl: PyType[OpVar2]) -> OpVar2:
        op = self[node].op
        assert isinstance(op, cl)
        return op

    def children(self, node: ToNode | None = None) -> list[Node]:
        node = node or self.root
        return self[node].children

    def _add_node(
        self,
        op: Op,
        parent: ToNode | None = None,
        num_outs: int | None = None,
    ) -> Node:
        parent = parent.to_node() if parent else None
        node_data = NodeData(op, parent)

        if self._free_nodes:
            node = self._free_nodes.pop()
            self._nodes[node.idx] = node_data
        else:
            node = Node(len(self._nodes))
            self._nodes.append(node_data)
        node = replace(node, _num_out_ports=num_outs)
        if parent:
            self[parent].children.append(node)
        return node

    def add_node(
        self,
        op: Op,
        parent: ToNode | None = None,
        num_outs: int | None = None,
    ) -> Node:
        parent = parent or self.root
        return self._add_node(op, parent, num_outs)

    def delete_node(self, node: ToNode) -> NodeData | None:
        node = node.to_node()
        parent = self[node].parent
        if parent:
            self[parent].children.remove(node)
        for offset in range(self.num_in_ports(node)):
            self._links.delete_right(_SubPort(node.inp(offset)))
        for offset in range(self.num_out_ports(node)):
            self._links.delete_left(_SubPort(node.out(offset)))

        weight, self._nodes[node.idx] = self._nodes[node.idx], None
        self._free_nodes.append(node)
        return weight

    def _unused_sub_offset(self, port: P) -> _SubPort[P]:
        d: dict[_SO, _SI] | dict[_SI, _SO]
        match port:
            case OutPort(_):
                d = self._links.fwd
            case InPort(_):
                d = self._links.bck
        sub_port = _SubPort(port)
        while sub_port in d:
            sub_port = sub_port.next_sub_offset()
        return sub_port

    def add_link(self, src: OutPort, dst: InPort) -> None:
        src_sub = self._unused_sub_offset(src)
        dst_sub = self._unused_sub_offset(dst)
        # if self._links.get_left(dst_sub) is not None:
        #     dst = replace(dst, _sub_offset=dst._sub_offset + 1)
        self._links.insert_left(src_sub, dst_sub)

        self[src.node]._num_outs = max(self[src.node]._num_outs, src.offset + 1)
        self[dst.node]._num_inps = max(self[dst.node]._num_inps, dst.offset + 1)

    def delete_link(self, src: OutPort, dst: InPort) -> None:
        try:
            sub_offset = next(
                i for i, inp in enumerate(self.linked_ports(src)) if inp == dst
            )
            self._links.delete_left(_SubPort(src, sub_offset))
        except StopIteration:
            return
        # TODO make sure sub-offset is handled correctly

    def root_op(self) -> OpVar:
        return cast(OpVar, self[self.root].op)

    def num_nodes(self) -> int:
        return len(self._nodes) - len(self._free_nodes)

    def num_ports(self, node: ToNode, direction: Direction) -> int:
        return (
            self.num_in_ports(node)
            if direction == Direction.INCOMING
            else self.num_out_ports(node)
        )

    def num_in_ports(self, node: ToNode) -> int:
        return self[node]._num_inps

    def num_out_ports(self, node: ToNode) -> int:
        return self[node]._num_outs

    def _linked_ports(
        self, port: P, links: dict[_SubPort[P], _SubPort[K]]
    ) -> Iterable[K]:
        sub_port = _SubPort(port)
        while sub_port in links:
            # sub offset not used in API
            yield links[sub_port].port
            sub_port = sub_port.next_sub_offset()

    @overload
    def linked_ports(self, port: OutPort) -> Iterable[InPort]: ...
    @overload
    def linked_ports(self, port: InPort) -> Iterable[OutPort]: ...
    def linked_ports(self, port: OutPort | InPort):
        match port:
            case OutPort(_):
                return self._linked_ports(port, self._links.fwd)
            case InPort(_):
                return self._linked_ports(port, self._links.bck)

    # TODO: single linked port

    def outgoing_order_links(self, node: ToNode) -> Iterable[Node]:
        return (p.node for p in self.linked_ports(node.out(-1)))

    def incoming_order_links(self, node: ToNode) -> Iterable[Node]:
        return (p.node for p in self.linked_ports(node.inp(-1)))

    def _node_links(
        self, node: ToNode, links: dict[_SubPort[P], _SubPort[K]]
    ) -> Iterable[tuple[P, list[K]]]:
        try:
            direction = next(iter(links.keys())).port.direction
        except StopIteration:
            return
        # iterate over known offsets
        for offset in range(self.num_ports(node, direction)):
            port = cast(P, node.port(offset, direction))
            yield port, list(self._linked_ports(port, links))

    def outgoing_links(self, node: ToNode) -> Iterable[tuple[OutPort, list[InPort]]]:
        return self._node_links(node, self._links.fwd)

    def incoming_links(self, node: ToNode) -> Iterable[tuple[InPort, list[OutPort]]]:
        return self._node_links(node, self._links.bck)

    def num_incoming(self, node: Node) -> int:
        # connecetd links
        return sum(1 for _ in self.incoming_links(node))

    def num_outgoing(self, node: ToNode) -> int:
        # connecetd links
        return sum(1 for _ in self.outgoing_links(node))

    # TODO: num_links and _linked_ports

    def port_kind(self, port: InPort | OutPort) -> Kind:
        return self[port.node].op.port_kind(port)

    def port_type(self, port: InPort | OutPort) -> Type | None:
        op = self[port.node].op
        if isinstance(op, DataflowOp):
            return op.port_type(port)
        return None

    def insert_hugr(self, hugr: Hugr, parent: ToNode | None = None) -> dict[Node, Node]:
        mapping: dict[Node, Node] = {}

        for idx, node_data in enumerate(hugr._nodes):
            if node_data is not None:
                # relies on parents being inserted before any children
                try:
                    node_parent = (
                        mapping[node_data.parent] if node_data.parent else parent
                    )
                except KeyError as e:
                    raise ParentBeforeChild() from e
                mapping[Node(idx)] = self.add_node(node_data.op, node_parent)

        for src, dst in hugr._links.items():
            self.add_link(
                mapping[src.port.node].out(src.port.offset),
                mapping[dst.port.node].inp(dst.port.offset),
            )
        return mapping

    def to_serial(self) -> SerialHugr:
        node_it = (node for node in self._nodes if node is not None)

        def _serialise_link(
            link: tuple[_SO, _SI],
        ) -> tuple[tuple[int, int], tuple[int, int]]:
            src, dst = link
            s, d = self._constrain_offset(src.port), self._constrain_offset(dst.port)
            return (src.port.node.idx, s), (dst.port.node.idx, d)

        return SerialHugr(
            version="v1",
            # non contiguous indices will be erased
            nodes=[node.to_serial(Node(idx), self) for idx, node in enumerate(node_it)],
            edges=[_serialise_link(link) for link in self._links.items()],
        )

    def _constrain_offset(self, p: P) -> int:
        # negative offsets are used to refer to the last port
        if p.offset < 0:
            match p.direction:
                case Direction.INCOMING:
                    current = self.num_incoming(p.node)
                case Direction.OUTGOING:
                    current = self.num_outgoing(p.node)
            offset = current + p.offset + 1
        else:
            offset = p.offset

        return offset

    @classmethod
    def from_serial(cls, serial: SerialHugr) -> Hugr:
        assert serial.nodes, "Empty Hugr is invalid"

        hugr = Hugr.__new__(Hugr)
        hugr._nodes = []
        hugr._links = BiMap()
        hugr._free_nodes = []
        hugr.root = Node(0)
        for idx, serial_node in enumerate(serial.nodes):
            parent: Node | None = Node(serial_node.root.parent)
            if serial_node.root.parent == idx:
                hugr.root = Node(idx)
                parent = None
            serial_node.root.parent = -1
            hugr._nodes.append(NodeData(serial_node.root.deserialize(), parent))

        for (src_node, src_offset), (dst_node, dst_offset) in serial.edges:
            if src_offset is None or dst_offset is None:
                continue
            hugr.add_link(
                Node(src_node).out(src_offset), Node(dst_node).inp(dst_offset)
            )

        return hugr
