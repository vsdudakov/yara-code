//! The two mermaid pictures a README actually draws — a pie and a flowchart —
//! read out of a fenced block and laid out in character cells.
//!
//! The layout is shared rather than done twice: the terminal paints the cells
//! as characters and the window multiplies them by the size of one, so a chart
//! has the same boxes in the same places in both frontends. Every other
//! mermaid diagram stays the code it was written as; a preview that renders a
//! sequence diagram wrong is worse than one that shows its source.

/// The outline a flowchart node is written with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// `id[Label]`
    Box,
    /// `id(Label)`
    Round,
    /// `id{Label}`
    Diamond,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    /// What `A -->|yes| B` writes on the arrow.
    pub label: Option<String>,
    /// A `-.->` arrow, drawn as a broken line.
    pub dashed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    /// `TD`/`TB`/`BT` run down the page; `LR`/`RL` run across it.
    pub down: bool,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slice {
    pub label: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Chart {
    Pie {
        title: Option<String>,
        slices: Vec<Slice>,
    },
    Flow(Flow),
}

/// Reads the body of a ```` ```mermaid ```` block. `None` for a diagram this
/// module does not draw, which leaves the block its source.
pub fn parse(body: &str) -> Option<Chart> {
    // Comments and blank lines say nothing about the picture.
    let lines: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"))
        .collect();
    let first = lines.first()?;
    let kind = first.split_whitespace().next()?;
    if kind.eq_ignore_ascii_case("pie") {
        return Some(pie(&lines));
    }
    if kind.eq_ignore_ascii_case("flowchart") || kind.eq_ignore_ascii_case("graph") {
        return Some(Chart::Flow(flow(&lines)));
    }
    None
}

/// `pie title Languages` / `title Languages`, then `"Rust" : 70` a line.
fn pie(lines: &[&str]) -> Chart {
    let mut title = after_title(lines[0]);
    let mut slices = Vec::new();
    for line in &lines[1..] {
        if let Some(text) = after_title(line) {
            if title.is_none() {
                title = Some(text);
            }
            continue;
        }
        let Some((label, value)) = line.rsplit_once(':') else {
            continue;
        };
        let label = label.trim().trim_matches('"').trim();
        if let Ok(value) = value.trim().parse::<f64>() {
            if !label.is_empty() && value > 0.0 {
                slices.push(Slice {
                    label: label.to_string(),
                    value,
                });
            }
        }
    }
    Chart::Pie { title, slices }
}

fn after_title(line: &str) -> Option<String> {
    let head = line.strip_prefix("pie").unwrap_or(line).trim_start();
    non_empty(head.strip_prefix("title ")?)
}

/// The arrows a flowchart is written with, longest first so that `-.->` is
/// never read as the `-->` inside it.
const ARROWS: [(&str, bool); 6] = [
    ("-.->", true),
    ("==>", false),
    ("-->", false),
    ("-.-", true),
    ("===", false),
    ("---", false),
];

fn flow(lines: &[&str]) -> Flow {
    let direction = lines[0].split_whitespace().nth(1).unwrap_or("TD");
    let down = !direction.eq_ignore_ascii_case("LR") && !direction.eq_ignore_ascii_case("RL");
    let mut flow = Flow {
        down,
        nodes: Vec::new(),
        edges: Vec::new(),
    };
    for line in &lines[1..] {
        // A subgraph is a grouping, not a node; its members are laid out with
        // everyone else rather than boxed together.
        if line.starts_with("subgraph") || *line == "end" || line.starts_with("style") {
            continue;
        }
        chain(&mut flow, line.trim_end_matches(';'));
    }
    flow
}

/// One line of a flowchart: `A --> B -->|label| C`, or a node on its own.
fn chain(flow: &mut Flow, line: &str) {
    // The line is cut at its arrows: the pieces between them are nodes, and
    // each arrow joins the node before it to the node after.
    let mut pieces: Vec<&str> = Vec::new();
    let mut arrows: Vec<(bool, Option<String>)> = Vec::new();
    let mut rest = line;
    while let Some((at, arrow, dashed)) = find_arrow(rest) {
        pieces.push(&rest[..at]);
        rest = &rest[at + arrow.len()..];
        // `A -->|yes| B` writes the arrow's label after it.
        let mut label = None;
        if let Some(inner) = rest.trim_start().strip_prefix('|') {
            if let Some((text, after)) = inner.split_once('|') {
                label = non_empty(text);
                rest = after;
            }
        }
        arrows.push((dashed, label));
    }
    pieces.push(rest);

    let mut nodes: Vec<Option<usize>> = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        // `A -- yes --> B` writes the label before the arrow instead. A label
        // never holds a bracket, which is what tells it from a node whose own
        // name has two dashes in it.
        let (piece, label) = match piece.rsplit_once("--") {
            Some((before, label)) if !label.contains(['[', '(', '{', ']', ')', '}']) => {
                (before, non_empty(label))
            }
            _ => (*piece, None),
        };
        if let (Some(label), Some(arrow)) = (label, arrows.get_mut(i)) {
            if arrow.1.is_none() {
                arrow.1 = Some(label);
            }
        }
        nodes.push(node(flow, piece));
    }
    for (i, (dashed, label)) in arrows.into_iter().enumerate() {
        join(flow, nodes[i], nodes[i + 1], label, dashed);
    }
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim().trim_matches('"').trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn join(
    flow: &mut Flow,
    from: Option<usize>,
    to: Option<usize>,
    label: Option<String>,
    dashed: bool,
) {
    if let (Some(from), Some(to)) = (from, to) {
        if from != to && !flow.edges.iter().any(|e| e.from == from && e.to == to) {
            flow.edges.push(Edge {
                from,
                to,
                label,
                dashed,
            });
        }
    }
}

fn find_arrow(text: &str) -> Option<(usize, &'static str, bool)> {
    let mut best: Option<(usize, &'static str, bool)> = None;
    for (arrow, dashed) in ARROWS {
        if let Some(at) = text.find(arrow) {
            if best.is_none_or(|(seen, _, _)| at < seen) {
                best = Some((at, arrow, dashed));
            }
        }
    }
    best
}

/// `id`, `id[Label]`, `id(Label)`, `id{Label}` — the node it names, added to
/// the chart the first time it is seen.
fn node(flow: &mut Flow, text: &str) -> Option<usize> {
    // A longer arrow than the three characters cut out of the line leaves a
    // dash or two behind on the piece beside it.
    let text = text.trim().trim_end_matches(';');
    let text = text.trim_matches(|c| "-.=>< ".contains(c));
    if text.is_empty() {
        return None;
    }
    let (id, label, shape) = match text.find(['[', '(', '{']) {
        Some(at) => {
            let shape = match text[at..].chars().next() {
                Some('(') => Shape::Round,
                Some('{') => Shape::Diamond,
                _ => Shape::Box,
            };
            let label = text[at..].trim_matches(|c| "[](){}<>/\\".contains(c));
            let label = label.trim().trim_matches('"').trim().to_string();
            let id = text[..at].trim().to_string();
            let id = if id.is_empty() { label.clone() } else { id };
            (id, label, shape)
        }
        None => (text.to_string(), text.to_string(), Shape::Box),
    };
    if let Some(seen) = flow.nodes.iter().position(|n| n.id == id) {
        // A node first met bare and named later keeps the name.
        if flow.nodes[seen].label == flow.nodes[seen].id && label != id {
            flow.nodes[seen].label = label;
            flow.nodes[seen].shape = shape;
        }
        return Some(seen);
    }
    flow.nodes.push(Node { id, label, shape });
    Some(flow.nodes.len() - 1)
}

// ----- layout ---------------------------------------------------------------

/// A node's box in character cells: three rows tall, as wide as its label and
/// the frame around it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub node: usize,
    pub x: usize,
    pub y: usize,
    pub w: usize,
}

/// One arrow, as the corners it turns. The head is on the last point.
#[derive(Clone, Debug, PartialEq)]
pub struct Wire {
    pub points: Vec<(usize, usize)>,
    pub label: Option<String>,
    pub dashed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagram {
    pub width: usize,
    pub height: usize,
    pub boxes: Vec<Placed>,
    pub wires: Vec<Wire>,
}

/// Every box is this tall: a border, its label, a border.
pub const BOX_H: usize = 3;
/// Between two boxes side by side, and between two ranks.
const GAP_ACROSS: usize = 4;
const GAP_DOWN: usize = 3;
/// The lane an arrow that loops backwards takes around the outside, and the
/// blank cell beyond it.
const LANE: usize = 3;

/// Places the nodes in ranks and routes the arrows between them. Ranks run
/// down the page or across it, whichever the chart asked for.
pub fn lay_out(flow: &Flow) -> Diagram {
    let count = flow.nodes.len();
    if count == 0 {
        return Diagram::default();
    }
    // A node sits one rank past the furthest node that reaches it. An arrow
    // that closes a loop says nothing about rank, and is left out of the sum.
    let loops = looping(flow);
    let mut rank = vec![0usize; count];
    for _ in 0..count {
        let mut moved = false;
        for (i, edge) in flow.edges.iter().enumerate() {
            if !loops[i] && edge.from != edge.to && rank[edge.to] <= rank[edge.from] {
                rank[edge.to] = rank[edge.from] + 1;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    let mut ranks: Vec<Vec<usize>> = vec![Vec::new(); rank.iter().copied().max().unwrap_or(0) + 1];
    for (node, rank) in rank.iter().enumerate() {
        ranks[*rank].push(node);
    }
    ranks.retain(|rank| !rank.is_empty());
    let depth = ranks.len();
    let width_of = |node: usize| flow.nodes[node].label.chars().count() + 4;
    let lane = loops.iter().any(|back| *back);

    let mut boxes: Vec<Placed> = Vec::new();
    let (mut width, mut height) = if flow.down {
        let widths: Vec<usize> = ranks
            .iter()
            .map(|rank| {
                rank.iter().map(|n| width_of(*n)).sum::<usize>()
                    + GAP_ACROSS * rank.len().saturating_sub(1)
            })
            .collect();
        let width = widths.iter().copied().max().unwrap_or(0);
        for (r, rank) in ranks.iter().enumerate() {
            // Ranks are centred on the same line rather than each on its own
            // edge, so an arrow between two of them runs straight down.
            let mut x = (width / 2).saturating_sub(widths[r] / 2);
            for node in rank {
                boxes.push(Placed {
                    node: *node,
                    x,
                    y: r * (BOX_H + GAP_DOWN),
                    w: width_of(*node),
                });
                x += width_of(*node) + GAP_ACROSS;
            }
        }
        (width, depth * (BOX_H + GAP_DOWN) - GAP_DOWN)
    } else {
        let heights: Vec<usize> = ranks
            .iter()
            .map(|rank| rank.len() * (BOX_H + 1) - 1)
            .collect();
        let height = heights.iter().copied().max().unwrap_or(0);
        let mut x = 0;
        for (r, rank) in ranks.iter().enumerate() {
            let column = rank.iter().map(|n| width_of(*n)).max().unwrap_or(0);
            let mut y = (height / 2).saturating_sub(heights[r] / 2);
            for node in rank {
                boxes.push(Placed {
                    node: *node,
                    x,
                    y,
                    w: width_of(*node),
                });
                y += BOX_H + 1;
            }
            x += column + GAP_ACROSS + 2;
        }
        let width = boxes
            .iter()
            .map(|placed| placed.x + placed.w)
            .max()
            .unwrap_or(0);
        (width, height)
    };
    // The lane the backwards arrows run in, outside every box.
    if lane {
        if flow.down {
            width += LANE;
        } else {
            height += LANE;
        }
    }

    let mut spot = vec![(0usize, 0usize, 0usize); count];
    for placed in &boxes {
        spot[placed.node] = (placed.x, placed.y, placed.w);
    }
    let wires = flow
        .edges
        .iter()
        .enumerate()
        .map(|(i, edge)| Wire {
            points: if loops[i] {
                around(flow.down, spot[edge.from], spot[edge.to], width, height)
            } else {
                route(flow.down, spot[edge.from], spot[edge.to])
            },
            label: edge.label.clone(),
            dashed: edge.dashed,
        })
        .collect();
    Diagram {
        width,
        height,
        boxes,
        wires,
    }
}

/// Which arrows close a loop, found by walking the chart from each node in
/// turn: an arrow onto a node the walk is already inside of is one of them.
fn looping(flow: &Flow) -> Vec<bool> {
    const UNSEEN: u8 = 0;
    const INSIDE: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![UNSEEN; flow.nodes.len()];
    let mut loops = vec![false; flow.edges.len()];
    let mut walk: Vec<(usize, usize)> = Vec::new();
    for start in 0..flow.nodes.len() {
        if state[start] != UNSEEN {
            continue;
        }
        state[start] = INSIDE;
        walk.push((start, 0));
        while let Some(&(node, from)) = walk.last() {
            let next = flow
                .edges
                .iter()
                .enumerate()
                .skip(from)
                .find(|(_, edge)| edge.from == node)
                .map(|(i, edge)| (i, edge.to));
            match next {
                Some((i, to)) => {
                    if let Some(top) = walk.last_mut() {
                        top.1 = i + 1;
                    }
                    match state[to] {
                        INSIDE => loops[i] = true,
                        UNSEEN => {
                            state[to] = INSIDE;
                            walk.push((to, 0));
                        }
                        _ => {}
                    }
                }
                None => {
                    state[node] = DONE;
                    walk.pop();
                }
            }
        }
    }
    loops
}

/// The corners an arrow turns between two boxes: out of one, along the gap
/// between the ranks, and into the other.
fn route(
    down: bool,
    from: (usize, usize, usize),
    to: (usize, usize, usize),
) -> Vec<(usize, usize)> {
    let (fx, fy, fw) = from;
    let (tx, ty, tw) = to;
    let mut points = if down {
        let (a, b) = (fx + fw / 2, tx + tw / 2);
        let (start, end) = if ty >= fy {
            ((a, fy + BOX_H), (b, ty.saturating_sub(1)))
        } else {
            ((a, fy.saturating_sub(1)), (b, ty + BOX_H))
        };
        let mid = (start.1 + end.1) / 2;
        vec![start, (a, mid), (b, mid), end]
    } else {
        let (a, b) = (fy + BOX_H / 2, ty + BOX_H / 2);
        let (start, end) = if tx >= fx {
            ((fx + fw, a), (tx.saturating_sub(1), b))
        } else {
            ((fx.saturating_sub(1), a), (tx + tw, b))
        };
        let mid = (start.0 + end.0) / 2;
        vec![start, (mid, a), (mid, b), end]
    };
    points.dedup();
    points
}

/// An arrow back to a rank already passed, taken around the outside of the
/// chart rather than through the boxes between.
fn around(
    down: bool,
    from: (usize, usize, usize),
    to: (usize, usize, usize),
    width: usize,
    height: usize,
) -> Vec<(usize, usize)> {
    let (fx, fy, fw) = from;
    let (tx, ty, tw) = to;
    let mut points = if down {
        let lane = width.saturating_sub(2);
        vec![
            (fx + fw, fy + BOX_H / 2),
            (lane, fy + BOX_H / 2),
            (lane, ty + BOX_H / 2),
            (tx + tw, ty + BOX_H / 2),
        ]
    } else {
        let lane = height.saturating_sub(2);
        vec![
            (fx + fw / 2, fy + BOX_H),
            (fx + fw / 2, lane),
            (tx + tw / 2, lane),
            (tx + tw / 2, ty + BOX_H),
        ]
    };
    points.dedup();
    points
}

// ----- drawing --------------------------------------------------------------

/// Which way a line leaves a cell. A cell knows the sides it is joined to
/// rather than the character it holds, so two arrows crossing or parting at
/// the same cell meet in a tee instead of one writing over the other.
const UP: u8 = 1;
const DOWN: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;

/// The characters a chart is drawn out of. The terminal falls back to the
/// ASCII set where the font has no box drawing.
struct Pen {
    unicode: bool,
}

impl Pen {
    /// The one character that joins the sides in `mask`.
    fn line(&self, mask: u8, dashed: bool) -> char {
        let across = mask & (LEFT | RIGHT) != 0;
        let down = mask & (UP | DOWN) != 0;
        if !self.unicode {
            return match (across, down) {
                (true, true) => '+',
                (_, true) => '|',
                _ => '-',
            };
        }
        match mask {
            m if m == UP | DOWN | LEFT | RIGHT => '┼',
            m if m == UP | DOWN | RIGHT => '├',
            m if m == UP | DOWN | LEFT => '┤',
            m if m == LEFT | RIGHT | DOWN => '┬',
            m if m == LEFT | RIGHT | UP => '┴',
            m if m == DOWN | RIGHT => '┌',
            m if m == DOWN | LEFT => '┐',
            m if m == UP | RIGHT => '└',
            m if m == UP | LEFT => '┘',
            _ if down && dashed => '┊',
            _ if down => '│',
            _ if dashed => '╌',
            _ => '─',
        }
    }

    fn head(&self, from: (usize, usize), to: (usize, usize)) -> char {
        let unicode = self.unicode;
        match (to.0.cmp(&from.0), to.1.cmp(&from.1)) {
            (std::cmp::Ordering::Greater, _) => {
                if unicode {
                    '▶'
                } else {
                    '>'
                }
            }
            (std::cmp::Ordering::Less, _) => {
                if unicode {
                    '◀'
                } else {
                    '<'
                }
            }
            (_, std::cmp::Ordering::Less) => {
                if unicode {
                    '▲'
                } else {
                    '^'
                }
            }
            _ => {
                if unicode {
                    '▼'
                } else {
                    'v'
                }
            }
        }
    }
}

/// Draws a flowchart as rows of characters — what the terminal renders, and
/// what the tests read.
pub fn draw(flow: &Flow, unicode: bool) -> Vec<String> {
    let pen = Pen { unicode };
    let diagram = lay_out(flow);
    if diagram.width == 0 || diagram.height == 0 {
        return Vec::new();
    }
    let mut joins = vec![vec![0u8; diagram.width]; diagram.height];
    let mut broken = vec![vec![false; diagram.width]; diagram.height];
    for wire in &diagram.wires {
        wire_on(&mut joins, &mut broken, wire);
    }
    let mut canvas: Vec<Vec<char>> = joins
        .iter()
        .enumerate()
        .map(|(y, row)| {
            row.iter()
                .enumerate()
                .map(|(x, mask)| match mask {
                    0 => ' ',
                    mask => pen.line(*mask, broken[y][x]),
                })
                .collect()
        })
        .collect();

    // The head goes on after the lines and the boxes after that: an arrow that
    // runs behind a box is hidden by it, which is how a reader takes it.
    for wire in &diagram.wires {
        if let (Some(last), Some(before)) = (wire.points.last(), wire.points.iter().nth_back(1)) {
            if let Some(cell) = canvas.get_mut(last.1).and_then(|row| row.get_mut(last.0)) {
                *cell = pen.head(*before, *last);
            }
        }
    }
    for placed in &diagram.boxes {
        box_on(&mut canvas, placed, &flow.nodes[placed.node], unicode);
    }
    for wire in &diagram.wires {
        if let Some(label) = &wire.label {
            label_on(&mut canvas, wire, label);
        }
    }
    let mut art: Vec<String> = canvas
        .into_iter()
        .map(|row| row.into_iter().collect::<String>().trim_end().to_string())
        .collect();
    while art.last().is_some_and(|row| row.is_empty()) {
        art.pop();
    }
    art
}

fn wire_on(joins: &mut [Vec<u8>], broken: &mut [Vec<bool>], wire: &Wire) {
    let path = path(wire);
    for pair in path.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let (out, back) = if to.0 > from.0 {
            (RIGHT, LEFT)
        } else if to.0 < from.0 {
            (LEFT, RIGHT)
        } else if to.1 > from.1 {
            (DOWN, UP)
        } else {
            (UP, DOWN)
        };
        for (cell, side) in [(from, out), (to, back)] {
            if let Some(mask) = joins.get_mut(cell.1).and_then(|row| row.get_mut(cell.0)) {
                *mask |= side;
                broken[cell.1][cell.0] = wire.dashed;
            }
        }
    }
}

/// Every cell an arrow runs through, corner to corner.
fn path(wire: &Wire) -> Vec<(usize, usize)> {
    let mut path: Vec<(usize, usize)> = Vec::new();
    for pair in wire.points.windows(2) {
        path.extend(cells_between(pair[0], pair[1]));
    }
    path.dedup();
    path
}

/// Every cell of a straight segment, both ends included.
fn cells_between(from: (usize, usize), to: (usize, usize)) -> Vec<(usize, usize)> {
    let (a, b) = (from.0.min(to.0), from.0.max(to.0));
    let (c, d) = (from.1.min(to.1), from.1.max(to.1));
    if from.1 == to.1 {
        if from.0 <= to.0 {
            (a..=b).map(|x| (x, from.1)).collect()
        } else {
            (a..=b).rev().map(|x| (x, from.1)).collect()
        }
    } else if from.1 <= to.1 {
        (c..=d).map(|y| (from.0, y)).collect()
    } else {
        (c..=d).rev().map(|y| (from.0, y)).collect()
    }
}

fn box_on(canvas: &mut [Vec<char>], placed: &Placed, node: &Node, unicode: bool) {
    let (top, side, corners) = if !unicode {
        ('-', '|', ['+', '+', '+', '+'])
    } else {
        match node.shape {
            Shape::Round => ('─', '│', ['╭', '╮', '╰', '╯']),
            Shape::Diamond => ('─', '◇', ['┌', '┐', '└', '┘']),
            Shape::Box => ('─', '│', ['┌', '┐', '└', '┘']),
        }
    };
    let inner = placed.w.saturating_sub(2);
    let label: Vec<char> = node.label.chars().collect();
    let rows = [
        std::iter::once(corners[0])
            .chain(std::iter::repeat_n(top, inner))
            .chain(std::iter::once(corners[1]))
            .collect::<Vec<char>>(),
        std::iter::once(side)
            .chain(std::iter::once(' '))
            .chain(label.iter().copied())
            .chain(std::iter::once(' '))
            .chain(std::iter::once(side))
            .collect(),
        std::iter::once(corners[2])
            .chain(std::iter::repeat_n(top, inner))
            .chain(std::iter::once(corners[3]))
            .collect(),
    ];
    for (dy, row) in rows.iter().enumerate() {
        for (dx, ch) in row.iter().enumerate() {
            if let Some(cell) = canvas
                .get_mut(placed.y + dy)
                .and_then(|line| line.get_mut(placed.x + dx))
            {
                *cell = *ch;
            }
        }
    }
}

/// An arrow's label, written beside where it leaves its box — to one side of
/// the line or the other, whichever has the room. It is left off rather than
/// drawn over anything: a chart with a label missing still reads, and one with
/// a box written through does not.
fn label_on(canvas: &mut [Vec<char>], wire: &Wire, label: &str) {
    let Some(start) = wire.points.first() else {
        return;
    };
    let text: Vec<char> = label.chars().collect();
    // The label goes on the side the arrow leaves towards, so the branch it
    // names is the one the reader's eye follows from it.
    let leftwards = wire.points.last().is_some_and(|last| last.0 < start.0);
    // A wire that leaves near the left edge has no room for a label on that
    // side, and a wire on the top row has none above it: those spots do not
    // exist, rather than wrapping round to the far side of the canvas.
    let right = Some(start.0 + 1);
    let left = start.0.checked_sub(text.len() + 1);
    let spots = [start.1.checked_sub(1), Some(start.1 + 1), Some(start.1)]
        .into_iter()
        .flatten()
        .flat_map(|y| {
            if leftwards {
                [(left, y), (right, y)]
            } else {
                [(right, y), (left, y)]
            }
        })
        .filter_map(|(x, y)| Some((x?, y)));
    let Some((x, y)) = spots.into_iter().find(|(x, y)| {
        canvas.get(*y).is_some_and(|row| {
            row.len() >= x + text.len() && row[*x..x + text.len()].iter().all(|c| *c == ' ')
        })
    }) else {
        return;
    };
    for (i, ch) in text.into_iter().enumerate() {
        canvas[y][x + i] = ch;
    }
}

/// What each slice is worth out of the whole, as a fraction — the one sum both
/// frontends would otherwise do for themselves.
pub fn shares(slices: &[Slice]) -> Vec<f64> {
    let total: f64 = slices.iter().map(|s| s.value).sum();
    if total <= 0.0 {
        return vec![0.0; slices.len()];
    }
    slices.iter().map(|s| s.value / total).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_on_a_wire_at_the_left_edge_is_placed_or_left_off_never_panics() {
        // A label longer than the room to the left of a wire that starts in
        // column 0 used to wrap its x round and index past the row.
        for source in [
            "graph TD\n A -->|yes| B",
            "graph LR\n A -->|a very long label indeed| B",
            "graph TD\n A -->|no| B\n B -->|yes| A",
        ] {
            let Some(Chart::Flow(flow)) = parse(source) else {
                panic!("a flow chart");
            };
            let _ = draw(&flow, true);
            let _ = draw(&flow, false);
        }
    }

    #[test]
    fn a_pie_reads_its_title_and_its_slices() {
        let chart = parse("pie title Languages\n  \"Rust\" : 70\n  \"Docs\" : 30").unwrap();
        let Chart::Pie { title, slices } = chart else {
            panic!("that is a pie");
        };
        assert_eq!(title.as_deref(), Some("Languages"));
        assert_eq!(slices[0].label, "Rust");
        assert_eq!(slices[1].value, 30.0);
        assert_eq!(shares(&slices), vec![0.7, 0.3]);
    }

    #[test]
    fn a_title_on_its_own_line_counts_too() {
        let chart = parse("pie showData\ntitle Share\nA : 1\nB : 1").unwrap();
        let Chart::Pie { title, slices } = chart else {
            panic!("that is a pie");
        };
        assert_eq!(title.as_deref(), Some("Share"));
        assert_eq!(slices.len(), 2);
    }

    #[test]
    fn a_flowchart_reads_its_nodes_arrows_and_direction() {
        let chart = parse("flowchart LR\n  A[Edit] --> B[Save]\n  B --> C{Ok?}").unwrap();
        let Chart::Flow(flow) = chart else {
            panic!("that is a flowchart");
        };
        assert!(!flow.down, "LR runs across the page");
        assert_eq!(flow.nodes.len(), 3);
        assert_eq!(flow.nodes[0].label, "Edit");
        assert_eq!(flow.nodes[2].shape, Shape::Diamond);
        assert_eq!(flow.edges.len(), 2);
        assert_eq!(flow.edges[1].from, 1);
        assert_eq!(flow.edges[1].to, 2);
    }

    #[test]
    fn an_arrow_carries_the_label_written_on_it() {
        let Chart::Flow(flow) = parse("graph TD\n A -->|yes| B\n A -- no --> C").unwrap() else {
            panic!("that is a flowchart");
        };
        assert_eq!(flow.edges[0].label.as_deref(), Some("yes"));
        assert_eq!(flow.edges[1].label.as_deref(), Some("no"));
        assert!(flow.down);
    }

    #[test]
    fn a_dotted_arrow_is_read_as_one() {
        let Chart::Flow(flow) = parse("graph LR\n A -.-> B").unwrap() else {
            panic!("that is a flowchart");
        };
        assert!(flow.edges[0].dashed);
    }

    #[test]
    fn a_diagram_this_module_cannot_draw_stays_its_source() {
        assert!(parse("sequenceDiagram\n A->>B: hi").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn ranks_run_down_the_page_or_across_it() {
        let Chart::Flow(down) = parse("graph TD\n A --> B").unwrap() else {
            panic!("flowchart");
        };
        let laid = lay_out(&down);
        assert_eq!(laid.boxes[0].y, 0);
        assert_eq!(laid.boxes[1].y, BOX_H + 3, "the second rank is below");
        assert_eq!(laid.boxes[0].x, laid.boxes[1].x, "and lines up with it");

        let Chart::Flow(across) = parse("graph LR\n A --> B").unwrap() else {
            panic!("flowchart");
        };
        let laid = lay_out(&across);
        assert_eq!(laid.boxes[0].y, laid.boxes[1].y);
        assert!(
            laid.boxes[1].x > laid.boxes[0].x,
            "the second rank is right"
        );
    }

    #[test]
    fn a_drawn_chart_frames_every_node_and_points_at_the_next() {
        let Chart::Flow(flow) = parse("graph LR\n A[Edit] --> B[Save]").unwrap() else {
            panic!("flowchart");
        };
        let art = draw(&flow, true);
        assert!(art.iter().any(|row| row.contains("Edit")));
        assert!(art.iter().any(|row| row.contains("Save")));
        assert!(art.iter().any(|row| row.contains('▶')), "{art:#?}");
        // The ASCII set draws the same picture with the characters a terminal
        // without box drawing has.
        let plain = draw(&flow, false);
        assert!(plain.iter().any(|row| row.contains('>')));
        assert!(plain.iter().all(|row| row.is_ascii()));
    }

    #[test]
    fn a_chain_of_arrows_joins_every_link_in_it() {
        let Chart::Flow(flow) = parse("graph TD\n A[One] --> B[Two] --> C[Three]").unwrap() else {
            panic!("flowchart");
        };
        assert_eq!(flow.nodes.len(), 3);
        assert_eq!(
            flow.edges
                .iter()
                .map(|e| (e.from, e.to))
                .collect::<Vec<_>>(),
            vec![(0, 1), (1, 2)]
        );
    }

    #[test]
    fn an_arrow_that_loops_back_takes_the_lane_around_the_outside() {
        let Chart::Flow(flow) = parse("graph LR\n A[One] --> B[Two]\n B --> A").unwrap() else {
            panic!("flowchart");
        };
        // The loop says nothing about rank: One is still the first box.
        let laid = lay_out(&flow);
        assert_eq!(laid.boxes.len(), 2);
        assert!(laid.boxes[1].x > laid.boxes[0].x);
        // And it is routed below both of them rather than back through them.
        let back = laid.wires.last().unwrap();
        let lowest = back.points.iter().map(|(_, y)| *y).max().unwrap();
        assert!(lowest > BOX_H, "under the boxes: {back:?}");
        let art = draw(&flow, true);
        assert!(art.iter().any(|row| row.contains('▲')), "{art:#?}");
    }

    #[test]
    fn a_chart_with_no_nodes_draws_nothing() {
        let flow = Flow {
            down: true,
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        assert!(draw(&flow, true).is_empty());
        assert_eq!(lay_out(&flow), Diagram::default());
    }
}
