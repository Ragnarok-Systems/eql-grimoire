//! THE DASHBOARD'S GRID MODEL: twelve columns, ten point rows, tiles placed by x and y.
//!
//! # THIS IS NOT A FLOW LAYOUT, AND THE FIRST VERSION OF THE DASHBOARD WAS ONE
//!
//! The mock, which is the owner's own page and is not in this tree, is a CSS grid:
//!
//! ```text
//! grid-template-columns: repeat(12, minmax(0, 1fr));
//! grid-auto-rows: 10px; column-gap: 12px; row-gap: 2px;
//! grid-column: var(--tile-column) / span var(--tile-span);
//! grid-row:    var(--tile-row)    / span var(--tile-rows);
//! ```
//!
//! Every tile has a COLUMN, a SPAN, a ROW and a HEIGHT in rows, and the reader drags one to a cell
//! and resizes it on both axes. The dashboard that shipped before this packed tiles into rows by
//! span alone, which is a different thing: a row of two wide tiles and one narrow one leaves a
//! hole, a tall tile beside a short one leaves a bigger hole, and nothing the reader did could
//! close either. The owner's words for it were that the mock "is a grid of x y and shit fits
//! inside, we don't waste space unless the user resizes it".
//!
//! # EVERYTHING HERE IS PURE AND THE POINTER ONLY CHOOSES ARGUMENTS
//!
//! Where a tile lands, whether it may land there, what it overlaps and how a saved file resolves
//! are all functions of a [`Cell`] and a list of them. The screen turns a pointer position into a
//! cell with [`cell_at`] and asks; nothing about the rules lives in paint code, so every rule a
//! reader can break by dragging is proven by a test rather than by looking.
//!
//! # A SAVED FILE FROM THE FLOW LAYOUT STILL LOADS
//!
//! The old `Slot` was `{ tile, span }`. [`Placement`] keeps those two names and adds `col`, `row`
//! and `rows` behind `serde(default)`, so a settings file from the old build deserialises with the
//! three new fields at zero and [`resolve`] notices: a zero row is "not placed yet" and is packed
//! under everything that is, at the tile's own default size. Nothing the reader arranged is lost
//! and nothing overlaps.
use egui::{Pos2, Rect, Vec2};
use serde::{Deserialize, Serialize};

/// How many columns the grid has. The mock's `repeat(12, …)`.
pub const COLS: u8 = 12;
/// One grid row, in points. The mock's `grid-auto-rows: 10px`.
/// HOW TALL ONE GRID ROW IS, top of one to top of the next.
///
/// THE PITCH AND NOT THE INK. A tile spanning N rows is N of these tall LESS one gutter, the
/// same way a tile spanning N columns is N column pitches less one [`COL_GAP`]. See
/// [`cell_rect`].
/// THE ROW PITCH A PAGE FALLS BACK TO when it has no height to divide.
///
/// # THE GRID WAS PROPORTIONAL ACROSS AND ABSOLUTE DOWN, AND THAT WAS THE WHOLE PROBLEM
///
/// A column is a twelfth of whatever width the page has, so the grid has always filled the
/// window horizontally whatever size it was. A row was a fixed twelve points, so the page was
/// a fixed height whatever window it was in: too tall for a quarter screen, which is what the
/// scrollbar was, and too short for a full one, which is what the band of dead page under the
/// last widget was. Every complaint the owner has made about the bottom of this page comes
/// back to that one asymmetry.
///
/// SO THE PITCH IS PASSED IN NOW, worked out by the page from the height it actually has:
/// [`row_height`]. This constant is what a caller uses when there is no page to measure,
/// which is the tests and nothing else.
pub const ROW_UNIT: f32 = 12.0;

/// THE PITCH THAT MAKES A LAYOUT OF `rows` ROWS EXACTLY FILL `height`.
///
/// FLOORED, SO A WINDOW TOO SHORT FOR ITS LAYOUT SCROLLS RATHER THAN CRUSHING IT. Under the
/// floor the page is taller than the viewport and the scroll area does its job, which is the
/// honest answer: clipping a card to nothing is worse than asking for a scroll.
pub fn row_height(height: f32, rows: u16) -> f32 {
    (height / f32::from(rows.max(1))).max(MIN_PITCH)
}

/// THE SHORTEST A ROW MAY BE SQUEEZED TO before the page gives up and scrolls instead.
///
/// Eight points is a fifth of a panel head: a layout whose rows are shorter than this is one
/// whose cards cannot draw their own titles.
pub const MIN_PITCH: f32 = 8.0;
/// Between columns. The mock's `column-gap: 12px`.
pub const COL_GAP: f32 = 12.0;
/// Between rows. The mock's `row-gap: 2px`.
/// THE GAP BETWEEN TWO TILES STACKED ON EACH OTHER.
///
/// # THE SAME AS [`COL_GAP`], BECAUSE THE OWNER IS RIGHT THAT THEY SHOULD MATCH
///
/// It was two points against twelve across, so cards sat a comfortable distance apart
/// side by side and almost touching top to bottom. That came from taking the mock's CSS
/// literally (`grid-auto-rows: 10px; row-gap: 2px`), where the ROW PITCH is what the two add
/// up to and the visible gutter is the smaller number. Twelve and twelve gives one gutter
/// everywhere, and the pitch is unchanged, so no tile moves and none of the bands re-cut:
/// every tile simply gives its last ten points back to the gap under it.
pub const ROW_GAP: f32 = 12.0;
/// THE SHORTEST A TILE MAY BE, in rows: enough for the panel head and one line under it.
///
/// A tile resized to one row is a tile whose head is cut off, and a head cut off is a tile the
/// reader can no longer find the handle on to fix it.
///
/// # SIX ROWS WAS THAT SUM WITH THREE TERMS LEFT OUT
///
/// Seventy points is the head (40) and thirty over. Out of that thirty come the panel's two
/// borders and `theme::panel_body`'s own margin, eleven points top and bottom, which leaves six
/// points of body: less than a third of a line. Every tile that draws a `+N more` line drew it
/// through its own bottom edge at this floor, which
/// `no_card_slices_a_row_at_any_height_a_reader_can_drag_it_to` reports by name.
///
/// EIGHT ROWS IS THE SAME SENTENCE WITH ALL THE TERMS IN IT: 94 points, less 40 of head, less 2
/// of border, less 22 of body margin, leaves 30, which is a line and the room to say what did not
/// fit. A tile that wants more says so in `Tile::min_rows`.
pub const MIN_ROWS: u16 = 8;
/// THE NARROWEST ANY TILE MAY BE, in columns.
///
/// # THIS WAS TWO, AND IT WAS A FLOOR UNDER EVERY WIDGET AT ONCE
///
/// Two columns is a sixth of the page. That is right for a roster, which has a five column
/// head, and wrong for a card holding one number: the owner spent three rounds asking for
/// `Written to disk` to be narrower, dragged at it himself, and the grid refused every time,
/// silently, because `Cell::clamped` widened it straight back. His words in the end were the
/// diagnosis: there are several kinds of widget with different minimums, and there needs to be
/// one half the size of the smallest.
///
/// SO THE GRID'S FLOOR IS ITS OWN UNIT, ONE COLUMN, and what a particular widget needs is the
/// WIDGET's business: see `Tile::min_span`, which is what the resize obeys, exactly as
/// `Tile::min_rows` already sat above [`MIN_ROWS`]. A floor in the geometry cannot know that a
/// roster needs room for five columns of head and a stat card needs room for two words.
pub const MIN_SPAN: u8 = 1;

/// WHERE ONE TILE SITS: `col`/`span` on the x axis, `row`/`rows` on the y axis, all one based.
///
/// One based because the mock's CSS is, and because a zero means "unplaced" in a saved file: see
/// the module note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub col: u8,
    pub span: u8,
    pub row: u16,
    pub rows: u16,
}

impl Cell {
    pub const fn new(col: u8, span: u8, row: u16, rows: u16) -> Cell {
        Cell {
            col,
            span,
            row,
            rows,
        }
    }

    /// The column after this tile's last, exclusive.
    pub fn col_end(self) -> u16 {
        u16::from(self.col) + u16::from(self.span)
    }

    /// The row after this tile's last, exclusive.
    pub fn row_end(self) -> u16 {
        self.row + self.rows
    }

    /// CLAMPED INTO THE GRID, keeping the size where it can and the position always.
    ///
    /// A span wider than the grid becomes the grid; a column that would push the tile off the
    /// right edge is pulled back so the whole tile is on screen. A zero row or column is left at
    /// zero on purpose: that is the "unplaced" marker [`resolve`] reads.
    pub fn clamped(self) -> Cell {
        let span = self.span.clamp(MIN_SPAN, COLS);
        let rows = self.rows.max(MIN_ROWS);
        let col = if self.col == 0 {
            0
        } else {
            self.col.min(COLS - span + 1)
        };
        Cell {
            col,
            span,
            row: self.row,
            rows,
        }
    }

    /// Is this a real position, or the "not placed yet" marker?
    pub fn placed(self) -> bool {
        self.col >= 1 && self.row >= 1
    }
}

/// DO TWO CELLS SHARE ANY GRID SQUARE? Edges touching is not overlap.
pub fn overlaps(a: Cell, b: Cell) -> bool {
    let x = u16::from(a.col) < b.col_end() && u16::from(b.col) < a.col_end();
    let y = a.row < b.row_end() && b.row < a.row_end();
    x && y
}

/// MAY `want` SIT HERE, given every other tile on the grid?
///
/// `except` is the index of the tile being moved, so it is not counted as blocking itself.
pub fn can_place(cells: &[Cell], except: Option<usize>, want: Cell) -> bool {
    if !want.placed() || want.col_end() > u16::from(COLS) + 1 {
        return false;
    }
    if want.span < MIN_SPAN || want.rows < MIN_ROWS {
        return false;
    }
    cells
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != except)
        .all(|(_, c)| !overlaps(*c, want))
}

/// THE FIRST ROW AT WHICH A TILE OF THIS SIZE FITS UNDER EVERYTHING ALREADY PLACED, at this column.
///
/// Used to auto-place a tile a saved file did not position and a tile just added from the
/// library. "Under everything" and not "in the first gap", because a reader who has arranged a
/// dashboard has a plan for the gaps in it, and a new tile appearing in the middle of that plan is
/// a tile appearing where he did not put it.
pub fn next_free_row(cells: &[Cell], _want_col: u8, _span: u8, _rows: u16) -> u16 {
    /* THE BOTTOM, FULL STOP. The first version of this walked down looking for the first row
     * the tile FITTED in, which is the first gap, which is exactly the behaviour the doc above
     * says this must not have. The test caught it on the mock's own layout: there is a hole under
     * the replay panel and a new tile was landing in it. */
    cells.iter().map(|c| c.row_end()).max().unwrap_or(1).max(1)
}

/// THE PIXEL RECT OF A CELL, given the grid's origin and one column's width.
pub fn cell_rect(origin: Pos2, colw: f32, rowh: f32, c: Cell) -> Rect {
    let x = origin.x + (f32::from(c.col) - 1.0) * (colw + COL_GAP);
    let w = f32::from(c.span) * colw + (f32::from(c.span) - 1.0) * COL_GAP;
    let y = origin.y + (f32::from(c.row) - 1.0) * rowh;
    /* N PITCHES LESS THE GUTTER UNDER IT, which is exactly what the width above computes
     * across. A tile is the room its rows take minus the space it owes the tile below. */
    let h = (f32::from(c.rows) * rowh - ROW_GAP).max(rowh);
    Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h))
}

/// The width of one column for a grid this wide.
pub fn col_width(full: f32) -> f32 {
    ((full - COL_GAP * (f32::from(COLS) - 1.0)) / f32::from(COLS)).max(8.0)
}

/// THE TOTAL HEIGHT OF THE GRID, in points: to the bottom of its lowest tile.
pub fn grid_height(cells: &[Cell], rowh: f32) -> f32 {
    let bottom = cells.iter().map(|c| c.row_end()).max().unwrap_or(1);
    let rows = bottom.saturating_sub(1);
    /* TO THE LOWEST TILE'S BOTTOM EDGE, AND NOT A GUTTER PAST IT.
     *
     * DEFECT: A BAND OF DEAD PAGE UNDER THE LAST WIDGET. A tile of N rows is N pitches LESS
     * the gutter it owes the tile below it (see [`cell_rect`]), and there is no tile below
     * the last one. Returning the full pitch left that gutter as scrollable nothing, and the
     * caller then added its own margin on top of it. The owner asked for the page to end where
     * the widgets end.
     */
    (f32::from(rows) * rowh - ROW_GAP).max(0.0)
}

/// WHICH CELL A POINT IS OVER: the column and row, one based, both clamped into the grid.
///
/// ROUNDED TO THE NEAREST CELL EDGE, not floored, so a tile dropped with its corner just short of
/// a column line still snaps to it. That is the difference between a drag that feels magnetic and
/// one that feels off by one.
pub fn cell_at(origin: Pos2, colw: f32, rowh: f32, p: Pos2) -> (u8, u16) {
    let fx = (p.x - origin.x) / (colw + COL_GAP);
    let fy = (p.y - origin.y) / rowh;
    let col = (fx.round() as i32 + 1).clamp(1, i32::from(COLS)) as u8;
    let row = (fy.round() as i32 + 1).max(1) as u16;
    (col, row)
}

/// WHICH PART OF A TILE'S BORDER IS BEING DRAGGED.
///
/// # ANY EDGE AND ANY CORNER, WHICH IS WHAT A RESIZE IS
///
/// The first version of this page had one grip, bottom right, and the owner's answer was that
/// you should be able to "grab the bottom and make it bigger, or the side, in any direction".
/// He is describing how every window on his desktop behaves, and a tile that resizes from one
/// corner only is a tile that has to be moved first to be made bigger on the left.
///
/// The four edges move one axis; the four corners move both. Pulling the LEFT or TOP edge moves
/// the tile's origin as well as its size, because that edge is where the origin is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Edge {
    /// Every edge, corners last, which is the order the page registers their hit zones in so a
    /// corner wins the pixels it shares with the two edges beside it.
    pub const ALL: [Edge; 8] = [
        Edge::N,
        Edge::S,
        Edge::E,
        Edge::W,
        Edge::NE,
        Edge::NW,
        Edge::SE,
        Edge::SW,
    ];

    /// +1 for the right side, -1 for the left, 0 for an edge that does not move that axis.
    pub fn dx(self) -> i32 {
        match self {
            Edge::E | Edge::NE | Edge::SE => 1,
            Edge::W | Edge::NW | Edge::SW => -1,
            Edge::N | Edge::S => 0,
        }
    }

    /// +1 for the bottom, -1 for the top, 0 for an edge that does not move that axis.
    pub fn dy(self) -> i32 {
        match self {
            Edge::S | Edge::SE | Edge::SW => 1,
            Edge::N | Edge::NE | Edge::NW => -1,
            Edge::E | Edge::W => 0,
        }
    }
}

/// THE CELL A TILE BECOMES WHEN THIS EDGE IS DRAGGED BY SO MANY COLUMNS AND ROWS.
///
/// `dcols` and `drows` are how far the pointer has moved from where the drag began, in whole
/// cells, right and down positive. The page turns pixels into those with [`delta_cells`].
///
/// # THE LEFT AND TOP EDGES MOVE THE ORIGIN, AND THAT IS THE WHOLE SUBTLETY
///
/// Dragging the right edge right by one is `span + 1`. Dragging the LEFT edge left by one is
/// `col - 1` AND `span + 1`: the far edge stays where it was. Written as a change to the origin
/// and a matching change to the size, so the far edge cannot drift, and clamped so the origin
/// never crosses the far edge minus the minimum size (a tile cannot be dragged inside out).
///
/// THE RIGHT AND BOTTOM EDGES ARE CLAMPED TO THE GRID AND TO THE MINIMUMS; the left and top
/// edges are clamped to column and row one. Nothing here knows about other tiles: whether the
/// result may actually sit there is [`can_place`]'s question, asked by the page on every frame
/// of the drag and answered by the colour of the ghost.
pub fn resize_from(c: Cell, edge: Edge, dcols: i32, drows: i32) -> Cell {
    let mut col = i32::from(c.col);
    let mut span = i32::from(c.span);
    let mut row = i32::from(c.row);
    let mut rows = i32::from(c.rows);
    let min_span = i32::from(MIN_SPAN);
    let min_rows = i32::from(MIN_ROWS);
    let cols = i32::from(COLS);
    match edge.dx() {
        1 => span += dcols,
        -1 => {
            let far = col + span; // exclusive right edge, fixed
            let new_col = (col + dcols).clamp(1, far - min_span);
            span = far - new_col;
            col = new_col;
        }
        _ => {}
    }
    match edge.dy() {
        1 => rows += drows,
        -1 => {
            let far = row + rows; // exclusive bottom edge, fixed
            let new_row = (row + drows).clamp(1, far - min_rows);
            rows = far - new_row;
            row = new_row;
        }
        _ => {}
    }
    span = span.clamp(min_span, (cols - col + 1).max(min_span));
    rows = rows.max(min_rows);
    Cell::new(col as u8, span as u8, row as u16, rows as u16)
}

/// HOW MANY WHOLE CELLS A POINTER HAS MOVED, right and down positive, from where a drag began.
///
/// Rounded, not floored, so the tile's edge follows the pointer to the nearest grid line rather
/// than trailing it by up to a cell.
pub fn delta_cells(colw: f32, rowh: f32, from: Pos2, to: Pos2) -> (i32, i32) {
    let dc = ((to.x - from.x) / (colw + COL_GAP)).round() as i32;
    let dr = ((to.y - from.y) / rowh).round() as i32;
    (dc, dr)
}

/// HOW MANY COLUMNS AND ROWS A PIXEL SIZE COVERS, for the resize grip.
pub fn size_at(colw: f32, rowh: f32, size: Vec2) -> (u8, u16) {
    let span = ((size.x + COL_GAP) / (colw + COL_GAP)).round();
    let rows = ((size.y + ROW_GAP) / rowh).round();
    (
        (span as i32).clamp(i32::from(MIN_SPAN), i32::from(COLS)) as u8,
        (rows as i32).max(i32::from(MIN_ROWS)) as u16,
    )
}

/* ------------------------------------------------------------------- what is saved -- */

/// ONE TILE IN THE SAVED DASHBOARD. `Settings::dashboard` is a list of these.
///
/// THE TILE IS A STRING ID, for the reason the old `Slot` used one: a settings file written by a
/// newer build with a tile this one does not have must still load, and an unknown id is dropped
/// rather than failing the whole file. See `dashboards::Tile::id`.
///
/// `col`, `row` AND `rows` DEFAULT TO ZERO, which is how a file from the flow layout loads: it had
/// `tile` and `span` only, so those two are read, the three new ones come back zero, and
/// [`resolve`] places the tile under everything that has a position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub tile: String,
    #[serde(default)]
    pub col: u8,
    pub span: u8,
    #[serde(default)]
    pub row: u16,
    #[serde(default)]
    pub rows: u16,
}

impl Placement {
    pub fn new(tile: &str, cell: Cell) -> Placement {
        Placement {
            tile: tile.to_owned(),
            col: cell.col,
            span: cell.span,
            row: cell.row,
            rows: cell.rows,
        }
    }

    pub fn cell(&self) -> Cell {
        Cell::new(self.col, self.span, self.row, self.rows)
    }

    pub fn set(&mut self, cell: Cell) {
        self.col = cell.col;
        self.span = cell.span;
        self.row = cell.row;
        self.rows = cell.rows;
    }
}

/// RESOLVE A SAVED LIST INTO CELLS THAT ARE ALL PLACED, IN THE GRID, AND NOT OVERLAPPING.
///
/// `fallback(id)` gives a tile's own default size for the ones that need placing (and its default
/// cell entirely, for a tile whose saved size is unusable). Returns `None` for an id the caller
/// does not know, and those entries are dropped.
///
/// THREE REPAIRS, EACH ONE A REAL CASE:
///
///   * A tile with no position (zero row or column: a file from the flow layout, or one added by
///     the library) goes under everything placed, at column 1, at its default size.
///   * A tile that overlaps one placed before it is moved down until it does not. The earlier
///     tile keeps its place because it is the one the reader arranged first; two tiles on one
///     square is a file that was edited by hand or by a bug, and the dashboard must still draw.
///   * A size outside the grid is clamped, before either of the above, so the repairs reason about
///     a tile that can exist.
///
/// # THE SAVED INDEX TRAVELS WITH EVERY ENTRY, AND IT DID NOT
///
/// This returned `(id, cell)` and the page wrote edits back with the DRAWN index. Those are the
/// same number only while every saved entry is a tile this build knows; the moment a file
/// carries an id from a newer build, that entry is dropped here, every drawn index after it is
/// one less than its saved index, and a move, a resize or an `x` lands on the wrong placement.
/// Three reviewers found it independently. The saved position is the first field now and the
/// page addresses the saved list with it and nothing else.
pub fn resolve<F>(saved: &[Placement], fallback: F) -> Vec<(usize, String, Cell)>
where
    F: Fn(&str) -> Option<Cell>,
{
    let mut out: Vec<(usize, String, Cell)> = Vec::with_capacity(saved.len());
    let mut cells: Vec<Cell> = Vec::with_capacity(saved.len());
    for (at, p) in saved.iter().enumerate() {
        let Some(default) = fallback(&p.tile) else {
            continue;
        };
        let mut c = p.cell();
        if c.span == 0 || c.rows == 0 {
            c.span = if c.span == 0 { default.span } else { c.span };
            c.rows = if c.rows == 0 { default.rows } else { c.rows };
        }
        let mut c = c.clamped();
        if !c.placed() {
            c.col = 1;
            c.row = next_free_row(&cells, 1, c.span, c.rows);
        }
        while !can_place(&cells, None, c) {
            c.row += 1;
            if c.row > 4096 {
                break;
            }
        }
        cells.push(c);
        out.push((at, p.tile.clone(), c));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DEFECT: A DASHBOARD THAT WASTED SPACE, because tiles had a width and nothing else.
    ///
    /// The mock places every tile by column, span, row and height; the first build of this page
    /// packed tiles into rows by span alone and could not express "this tall one beside those two
    /// short ones". Every rule below is one a reader can hit by dragging.
    ///
    /// WHAT MUTATION MAKES THIS RED: an overlap test that only checks one axis, a `can_place` that
    /// lets a tile off the right edge, or `cell_rect` dropping a gap.
    #[test]
    fn two_tiles_overlap_only_when_they_share_a_square_on_both_axes() {
        let a = Cell::new(1, 7, 1, 31);
        let beside = Cell::new(8, 5, 1, 22);
        let under = Cell::new(1, 4, 41, 14);
        let on_top = Cell::new(4, 4, 20, 10);
        assert!(
            !overlaps(a, beside),
            "sharing rows but not columns is not an overlap"
        );
        assert!(
            !overlaps(a, under),
            "sharing columns but not rows is not an overlap"
        );
        assert!(overlaps(a, on_top), "sharing both axes is");
        /* EDGES TOUCHING IS NOT OVERLAP, which is the whole of what makes a packed grid packable. */
        assert!(!overlaps(Cell::new(1, 6, 1, 10), Cell::new(7, 6, 1, 10)));
        assert!(!overlaps(Cell::new(1, 6, 1, 10), Cell::new(1, 6, 11, 10)));
    }

    #[test]
    fn a_tile_may_not_leave_the_grid_or_land_on_another() {
        let cells = vec![Cell::new(1, 7, 1, 31), Cell::new(8, 5, 1, 22)];
        assert!(
            can_place(&cells, None, Cell::new(8, 5, 23, 18)),
            "a free square"
        );
        assert!(
            !can_place(&cells, None, Cell::new(2, 4, 5, 10)),
            "on the first tile"
        );
        assert!(
            !can_place(&cells, None, Cell::new(10, 5, 40, 10)),
            "off the right edge: column 10 plus span 5 ends at 15, past 13"
        );
        assert!(
            can_place(&cells, Some(0), Cell::new(2, 4, 5, 10)),
            "the tile being moved does not block itself"
        );
        assert!(
            !can_place(&cells, None, Cell::new(0, 4, 40, 10)),
            "unplaced is not a place"
        );
        /* ZERO COLUMNS AND NOT ONE. `MIN_SPAN` is one now, which is the grid's own unit: a
         * tile a single column wide is a real tile and `Tile::min_span` is what says whether a
         * particular widget may be one. What can never be placed is a tile of no width at all. */
        assert!(
            !can_place(&cells, None, Cell::new(1, 0, 40, 10)),
            "narrower than MIN_SPAN"
        );
        assert!(
            can_place(&cells, None, Cell::new(1, 1, 40, 10)),
            "one column is a width the grid can place; a widget refuses it, not the geometry"
        );
        assert!(
            !can_place(&cells, None, Cell::new(1, 4, 40, 2)),
            "shorter than MIN_ROWS: a head with no body and no handle"
        );
    }

    #[test]
    fn the_pixel_rect_of_a_cell_honours_both_gaps() {
        let origin = Pos2::new(100.0, 50.0);
        let colw = 60.0;
        /* ONE ROW IS ONE PITCH LESS THE GUTTER IT OWES THE TILE UNDER IT, floored at the pitch
         * so a one row tile is not negative. */
        let r = cell_rect(origin, colw, ROW_UNIT, Cell::new(1, 1, 1, 1));
        assert_eq!(r.min, origin);
        assert_eq!(r.size(), Vec2::new(60.0, ROW_UNIT));
        /* SPAN 2 INCLUDES ONE COLUMN GAP; ROWS 3 IS THREE PITCHES LESS ONE ROW GAP. */
        let r = cell_rect(origin, colw, ROW_UNIT, Cell::new(3, 2, 4, 3));
        assert_eq!(r.min.x, 100.0 + 2.0 * (60.0 + COL_GAP));
        assert_eq!(r.width(), 2.0 * 60.0 + COL_GAP);
        assert_eq!(r.min.y, 50.0 + 3.0 * ROW_UNIT);
        assert_eq!(r.height(), 3.0 * ROW_UNIT - ROW_GAP);

        /* AND THE TWO GUTTERS ARE THE SAME, which is the whole point of the change: the space
         * between two tiles side by side and two stacked is one number. */
        let a = cell_rect(origin, colw, ROW_UNIT, Cell::new(1, 1, 1, 4));
        let below = cell_rect(origin, colw, ROW_UNIT, Cell::new(1, 1, 5, 4));
        let beside = cell_rect(origin, colw, ROW_UNIT, Cell::new(2, 1, 1, 4));
        assert_eq!(
            below.top() - a.bottom(),
            beside.left() - a.right(),
            "the gap under a tile and the gap beside it are different sizes"
        );
        /* TWELVE COLUMNS FILL THE WIDTH EXACTLY, gaps included. */
        let full = 12.0 * colw + 11.0 * COL_GAP;
        assert_eq!(col_width(full), colw);
        let last = cell_rect(origin, colw, ROW_UNIT, Cell::new(12, 1, 1, 1));
        assert!((last.right() - (origin.x + full)).abs() < 0.01);
    }

    #[test]
    fn a_point_snaps_to_the_nearest_cell_and_never_off_the_grid() {
        let origin = Pos2::new(0.0, 0.0);
        let colw = 60.0;
        assert_eq!(cell_at(origin, colw, ROW_UNIT, Pos2::new(0.0, 0.0)), (1, 1));
        /* JUST SHORT OF THE SECOND COLUMN LINE STILL SNAPS TO IT. */
        assert_eq!(
            cell_at(origin, colw, ROW_UNIT, Pos2::new(68.0, 0.0)),
            (2, 1)
        );
        assert_eq!(
            cell_at(origin, colw, ROW_UNIT, Pos2::new(-500.0, -500.0)),
            (1, 1)
        );
        assert_eq!(
            cell_at(origin, colw, ROW_UNIT, Pos2::new(5000.0, 0.0)).0,
            COLS
        );
        /* ROW 12 POINTS DOWN IS THE SECOND ROW. */
        assert_eq!(
            cell_at(origin, colw, ROW_UNIT, Pos2::new(0.0, 12.0)),
            (1, 2)
        );
        /* AND A SIZE IN PIXELS ROUNDS TO WHOLE CELLS, floored at the minimums. */
        /* 120 POINTS IS TEN PITCHES, and a tile of ten rows is ten pitches less the gutter, so
         * a drag to 120 lands on eleven rows rather than ten. The arithmetic is `size_at`'s own
         * inverse of `cell_rect`. */
        assert_eq!(
            size_at(colw, ROW_UNIT, Vec2::new(2.0 * 60.0 + COL_GAP, 120.0)),
            (2, 11)
        );
        assert_eq!(
            size_at(colw, ROW_UNIT, Vec2::new(1.0, 1.0)),
            (MIN_SPAN, MIN_ROWS)
        );
    }

    /// DEFECT: A SAVED FILE FROM THE OLD FLOW LAYOUT THAT LOSES THE READER'S DASHBOARD.
    ///
    /// The old `Slot` was `{tile, span}`. It must load, and every tile in it must end up placed,
    /// in the grid, and not on top of another. Two overlapping tiles from a hand edited file must
    /// also both draw.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `placed()` repair, dropping the overlap repair,
    /// or resolving an unknown id into a tile.
    #[test]
    fn a_saved_layout_is_repaired_into_a_grid_that_can_be_drawn() {
        let fallback = |id: &str| match id {
            "a" => Some(Cell::new(1, 7, 1, 31)),
            "b" => Some(Cell::new(8, 5, 1, 22)),
            "c" => Some(Cell::new(1, 4, 41, 14)),
            _ => None,
        };
        /* AN OLD FILE: spans only. Both get placed, one under the other, nothing overlaps. */
        let old = vec![
            Placement {
                tile: "a".into(),
                col: 0,
                span: 7,
                row: 0,
                rows: 0,
            },
            Placement {
                tile: "b".into(),
                col: 0,
                span: 5,
                row: 0,
                rows: 0,
            },
            Placement {
                tile: "from-the-future".into(),
                col: 1,
                span: 3,
                row: 1,
                rows: 10,
            },
        ];
        let got = resolve(&old, fallback);
        assert_eq!(got.len(), 2, "the unknown tile was not dropped");
        for (_, id, c) in &got {
            assert!(c.placed(), "{id} was left unplaced");
            assert!(
                c.col_end() <= u16::from(COLS) + 1,
                "{id} hangs off the grid"
            );
        }
        assert!(!overlaps(got[0].2, got[1].2), "the repaired tiles overlap");
        /* AND EACH KNOWS WHERE IT CAME FROM. Put the unknown entry FIRST and the drawn tile
         * is saved entry 1: a page writing back by drawn index would edit the unknown one. */
        assert_eq!((got[0].0, got[1].0), (0, 1));
        let unknown_first = vec![
            Placement::new("from-the-future", Cell::new(1, 3, 1, 10)),
            Placement::new("a", Cell::new(1, 7, 1, 31)),
        ];
        let got = resolve(&unknown_first, fallback);
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].0, 1,
            "the drawn tile does not know it is saved entry 1"
        );

        /* A HAND EDITED FILE WITH TWO TILES ON ONE SQUARE: the second is moved down. */
        let clash = vec![
            Placement::new("a", Cell::new(1, 6, 1, 10)),
            Placement::new("b", Cell::new(1, 6, 1, 10)),
        ];
        let got = resolve(&clash, fallback);
        assert_eq!(
            got[0].2,
            Cell::new(1, 6, 1, 10),
            "the first placed tile keeps its place"
        );
        assert_eq!(got[1].2.row, 11, "the second was not moved under the first");

        /* A SPAN WIDER THAN THE GRID IS CLAMPED, AND A ZERO SIZE TAKES THE DEFAULT. */
        let wide = vec![Placement {
            tile: "c".into(),
            col: 5,
            span: 40,
            row: 3,
            rows: 0,
        }];
        let got = resolve(&wide, fallback);
        assert_eq!(got[0].2.span, COLS);
        assert_eq!(
            got[0].2.col, 1,
            "a full width tile can only sit at column 1"
        );
        assert_eq!(
            got[0].2.rows, 14,
            "a zero height took the tile's own default"
        );
    }

    /// DEFECT: A TILE THAT COULD ONLY BE MADE BIGGER FROM ONE CORNER.
    ///
    /// The owner: "we should be able to grab the bottom and make it bigger, or the side, in any
    /// direction". Every edge and every corner, and the ones on the left and top move the ORIGIN
    /// while the far edge stays put, which is the part a one-corner grip never had to get right.
    ///
    /// WHAT MUTATION MAKES THIS RED: a left or top drag that moves the origin without moving the
    /// size (the far edge drifts), a clamp that lets a tile be dragged inside out, a right edge
    /// that can leave the grid, or an edge that moves the wrong axis.
    #[test]
    fn a_tile_resizes_from_any_edge_and_the_far_edge_stays_put() {
        let c = Cell::new(4, 4, 10, 10); // columns 4..8, rows 10..20

        /* THE RIGHT EDGE: span only. */
        assert_eq!(resize_from(c, Edge::E, 2, 0), Cell::new(4, 6, 10, 10));
        assert_eq!(
            resize_from(c, Edge::E, 0, 5),
            c,
            "a horizontal edge ignores rows"
        );
        /* THE BOTTOM EDGE: rows only. */
        assert_eq!(resize_from(c, Edge::S, 0, 3), Cell::new(4, 4, 10, 13));
        assert_eq!(
            resize_from(c, Edge::S, 5, 0),
            c,
            "a vertical edge ignores columns"
        );

        /* THE LEFT EDGE MOVES THE ORIGIN AND THE FAR EDGE STAYS AT 8. */
        let w = resize_from(c, Edge::W, -2, 0);
        assert_eq!(w, Cell::new(2, 6, 10, 10));
        assert_eq!(w.col_end(), c.col_end(), "the right edge drifted");
        /* AND PULLING IT INWARDS SHRINKS FROM THE LEFT. */
        let w = resize_from(c, Edge::W, 1, 0);
        assert_eq!(w, Cell::new(5, 3, 10, 10));
        assert_eq!(w.col_end(), c.col_end());
        /* THE TOP EDGE, the same on the other axis. */
        let n = resize_from(c, Edge::N, 0, -4);
        assert_eq!(n, Cell::new(4, 4, 6, 14));
        assert_eq!(n.row_end(), c.row_end(), "the bottom edge drifted");

        /* THE CORNERS MOVE BOTH AXES. */
        assert_eq!(resize_from(c, Edge::SE, 1, 2), Cell::new(4, 5, 10, 12));
        let nw = resize_from(c, Edge::NW, -1, -2);
        assert_eq!(nw, Cell::new(3, 5, 8, 12));
        assert_eq!((nw.col_end(), nw.row_end()), (c.col_end(), c.row_end()));
        assert_eq!(resize_from(c, Edge::NE, 2, -2), Cell::new(4, 6, 8, 12));
        assert_eq!(resize_from(c, Edge::SW, -1, 3), Cell::new(3, 5, 10, 13));

        /* A TILE CANNOT BE DRAGGED INSIDE OUT: the left edge stops at MIN_SPAN from the right. */
        let inside_out = resize_from(c, Edge::W, 40, 0);
        assert_eq!(inside_out.span, MIN_SPAN);
        assert_eq!(inside_out.col_end(), c.col_end());
        let inside_out = resize_from(c, Edge::N, 0, 40);
        assert_eq!(inside_out.rows, MIN_ROWS);
        assert_eq!(inside_out.row_end(), c.row_end());
        /* AND NOT SMALLER THAN THE MINIMUM FROM THE OTHER SIDE EITHER. */
        assert_eq!(resize_from(c, Edge::E, -40, 0).span, MIN_SPAN);
        assert_eq!(resize_from(c, Edge::S, 0, -40).rows, MIN_ROWS);

        /* THE RIGHT EDGE STOPS AT THE GRID; THE LEFT AND TOP STOP AT ONE. */
        let wide = resize_from(c, Edge::E, 40, 0);
        assert_eq!(
            wide.col_end(),
            u16::from(COLS) + 1,
            "the right edge left the grid"
        );
        let left = resize_from(c, Edge::W, -40, 0);
        assert_eq!(left.col, 1);
        assert_eq!(left.col_end(), c.col_end());
        let top = resize_from(c, Edge::N, 0, -40);
        assert_eq!(top.row, 1);
        assert_eq!(top.row_end(), c.row_end());

        /* AND PIXELS BECOME CELLS BY ROUNDING TO THE NEAREST LINE. */
        let colw = 60.0;
        assert_eq!(
            delta_cells(colw, ROW_UNIT, Pos2::new(0.0, 0.0), Pos2::new(140.0, -25.0)),
            (2, -2)
        );
        assert_eq!(
            delta_cells(colw, ROW_UNIT, Pos2::new(0.0, 0.0), Pos2::new(30.0, 5.0)),
            (0, 0)
        );
    }

    #[test]
    fn a_new_tile_goes_under_everything_and_not_into_a_gap_the_reader_left() {
        let cells = vec![Cell::new(1, 7, 1, 31), Cell::new(8, 5, 1, 22)];
        /* THERE IS A GAP under the second tile (rows 23..31, columns 8..12) and a new tile that
         * would fit there must not take it. */
        assert_eq!(next_free_row(&cells, 8, 5, 8), 32);
        /* THE LOWEST TILE'S OWN BOTTOM, which is its pitches less the gutter it does not owe. */
        assert_eq!(grid_height(&cells, ROW_UNIT), 31.0 * ROW_UNIT - ROW_GAP);

        /* AND IT IS EXACTLY THAT TILE'S BOTTOM, asked of `cell_rect` rather than restated. A
         * page that allocates more than this scrolls past its own contents. */
        let origin = Pos2::ZERO;
        let lowest = cells
            .iter()
            .max_by_key(|c| c.row_end())
            .copied()
            .expect("a cell");
        assert!(
            (cell_rect(origin, 60.0, ROW_UNIT, lowest).bottom() - grid_height(&cells, ROW_UNIT))
                .abs()
                < 0.01,
            "the grid is taller than the tile at the bottom of it"
        );
        assert_eq!(next_free_row(&[], 1, 4, 10), 1);
    }
}
