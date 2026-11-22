# Where the algorithms come from

The layout and routing are implemented from published papers only. No graph-layout library's
source (Graphviz, dagre, ELK, libavoid, Dunnart, yFiles or any other) was read.

- Sugiyama, Tagawa and Toda, "Methods for Visual Understanding of Hierarchical System
  Structures", IEEE Transactions on Systems, Man and Cybernetics 11(2), 1981: the layered
  pipeline.
- Eades, Lin and Smyth, "A fast and effective heuristic for the feedback arc set problem",
  Information Processing Letters 47(6), 1993: cycle removal.
- Gansner, Koutsofios, North and Vo, "A Technique for Drawing Directed Graphs", IEEE
  Transactions on Software Engineering 19(3), 1993: network simplex ranking (section 2) and
  weighted median crossing reduction with transposition (section 3).
- Brandes and Köpf, "Fast and Simple Horizontal Coordinate Assignment", Graph Drawing 2001:
  coordinates across the ranks.
- Sander, "Layout of compound directed graphs", Technical Report A/03/96, Universität des
  Saarlandes, 1996: groups kept contiguous and edges into group borders.
- Wybrow, Marriott and Stuckey, "Orthogonal Connector Routing", Graph Drawing 2009:
  orthogonal visibility graph, A* search with a bend penalty, and nudging.

The module docs cite the section each step follows. Tests use graphs written by us; no fixture
from another tool is used.
