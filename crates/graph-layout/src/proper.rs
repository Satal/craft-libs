//! The proper layered graph: every edge spans one rank, long edges run through dummy nodes and
//! each group is bracketed by a left and a right border node on every rank it spans.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A real node (or an empty group's placeholder), by node index.
    Node(usize),
    /// A bend point of a long edge.
    Dummy,
    /// The dummy that carries an edge's label.
    Label(usize),
    /// The left border of a group on one rank.
    BorderL(usize),
    /// The right border of a group on one rank.
    BorderR(usize),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Item {
    pub kind: Kind,
    pub rank: usize,
    /// The innermost group the item sits in (for borders: the group they bracket).
    pub owner: Option<usize>,
    /// Extent from the centre towards lower cross coordinates.
    pub left_ext: f64,
    /// Extent from the centre towards higher cross coordinates.
    pub right_ext: f64,
    /// Size along the rank axis.
    pub along: f64,
}

impl Item {
    pub fn is_border(&self) -> bool {
        matches!(self.kind, Kind::BorderL(_) | Kind::BorderR(_))
    }
    pub fn is_dummy(&self) -> bool {
        matches!(self.kind, Kind::Dummy | Kind::Label(_))
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Proper {
    pub items: Vec<Item>,
    /// Neighbours on the rank above, with multiplicity.
    pub up: Vec<Vec<usize>>,
    /// Neighbours on the rank below, with multiplicity.
    pub down: Vec<Vec<usize>>,
    /// Items per rank, in their current order.
    pub layers: Vec<Vec<usize>>,
    /// Parent of each group (component-local).
    pub group_parent: Vec<Option<usize>>,
}

impl Proper {
    pub fn push(&mut self, item: Item) -> usize {
        let id = self.items.len();
        self.items.push(item);
        self.up.push(Vec::new());
        self.down.push(Vec::new());
        if self.layers.len() <= item.rank {
            self.layers.resize(item.rank + 1, Vec::new());
        }
        self.layers[item.rank].push(id);
        id
    }

    /// Adds a segment from `a` (rank r) to `b` (rank r + 1).
    pub fn link(&mut self, a: usize, b: usize) {
        self.down[a].push(b);
        self.up[b].push(a);
    }

    /// Position of each item within its layer.
    pub fn positions(&self) -> Vec<usize> {
        let mut pos = vec![0; self.items.len()];
        for layer in &self.layers {
            for (i, &v) in layer.iter().enumerate() {
                pos[v] = i;
            }
        }
        pos
    }

    /// Whether the segment from `a` (upper) to `b` (lower) should stay straight: both ends are
    /// edge dummies, or both are the same group's border on neighbouring ranks.
    pub fn inner(&self, a: usize, b: usize) -> bool {
        let (ka, kb) = (self.items[a].kind, self.items[b].kind);
        match (ka, kb) {
            (Kind::BorderL(g), Kind::BorderL(h)) | (Kind::BorderR(g), Kind::BorderR(h)) => g == h,
            _ => self.items[a].is_dummy() && self.items[b].is_dummy(),
        }
    }
}
