//! The parts of a device card a view names, drawn with what draws the built-in cards: the
//! display at its left, or its rows of steps there, and its cells in two rows, of which the first
//! columns show and the rest wait behind the expand icon of the card.

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ElementId, Empty, GlobalElementId, InspectorElementId,
    LayoutId, Pixels, SharedString, Size, Window, div, point, prelude::*, px, size,
};
use sound_ui::ControlSize;
use sound_ui::components::cell::{CELL_WIDTH, CONTROL_HEIGHT, Cell, ROW_HEIGHT};
use sound_ui::components::device_card::Section;
use sound_ui::components::display::{self, Display};

use crate::tree::{Axes, CellProps, DisplayProps, Layout, List, Mark, NodeTable, Props};

use super::drawing::Drawing;

/// From the edges of a display to what its view draws in it, as the waveforms of the built-in
/// cards keep: a line at 0 or 1 shows whole, and a handle there is not cut in half.
const DISPLAY_MARGIN: f32 = 6.;
/// The columns of cells a card shows before its expand icon: beside a display or rows of steps,
/// as the built-in cards, and alone.
const COLUMNS_BESIDE_DISPLAY: usize = 2;
const COLUMNS_ALONE: usize = 4;

/// The children of a node that draw.
pub(super) fn drawn<'a>(
    table: &'a NodeTable,
    children: &'a [usize],
) -> impl Iterator<Item = usize> + 'a {
    (children.iter().copied()).filter(|child| {
        !matches!(
            table.get(*child).map(|child| &child.props),
            Some(Props::Marker) | None
        )
    })
}

/// Where the zero of a wave of a scope among `shapes` lies down the drawing it fills: the
/// display draws its zero line there by itself.
fn scope_zero(table: &NodeTable, shapes: &[usize]) -> Option<f32> {
    shapes
        .iter()
        .find_map(|slot| match &table.get(*slot)?.props {
            Props::Shape(shape)
                if matches!(shape.list, Some(List::Scope { .. }))
                    && shape.y.is_none()
                    && shape.height.is_none() =>
            {
                let (low, high) = (shape.low.unwrap_or(-1.), shape.high.unwrap_or(1.));
                (low < 0. && high > 0.).then(|| high / (high - low))
            }
            _ => None,
        })
}

/// How many cells of each `Controls` a card shows before its expand icon.
fn shown_cells(table: &NodeTable) -> usize {
    let display =
        (table.entries()).any(|entry| matches!(entry.props, Props::Display(_) | Props::Steps(_)));
    2 * if display {
        COLUMNS_BESIDE_DISPLAY
    } else {
        COLUMNS_ALONE
    }
}

/// Whether cells of a card wait behind its expand icon.
pub(super) fn hides_cells(table: &NodeTable) -> bool {
    let shown = shown_cells(table);
    (table.entries()).any(|entry| {
        matches!(entry.props, Props::Controls) && drawn(table, &entry.children).count() > shown
    })
}

impl Drawing<'_> {
    /// The parts of a card side by side: its rows of steps one over the other where a display
    /// is, then its display and its cells.
    pub(super) fn card_row(&mut self, children: &[usize]) -> AnyElement {
        let table = self.table;
        let is_steps = |slot: &usize| {
            let props = table.get(*slot).map(|entry| &entry.props);
            matches!(props, Some(Props::Steps(_)))
        };
        let (steps, rest): (Vec<usize>, Vec<usize>) = drawn(table, children).partition(is_steps);
        let rows = (!steps.is_empty())
            .then(|| (self.step_rows(&steps, &[]).justify_center()).mr(px(display::GAP)));
        (div().flex().flex_none().h(px(ROW_HEIGHT * 2.)))
            .children(rows)
            .children(rest.into_iter().map(|part| self.draw(part)))
            .into_any_element()
    }

    /// Draws with the controls that name no size small, as a card has them.
    pub(super) fn small<T>(&mut self, draw: impl FnOnce(&mut Self) -> T) -> T {
        let outer = std::mem::replace(&mut self.control_size, ControlSize::Sm);
        let drawn = draw(self);
        self.control_size = outer;
        drawn
    }

    pub(super) fn display(
        &mut self,
        id: u64,
        props: &DisplayProps,
        children: &[usize],
    ) -> AnyElement {
        let table = self.table;
        let wide = props.wide == Some(true);
        let name = self.element_name("display", None, id);
        let is_space = |slot: &usize| {
            let props = table.get(*slot).map(|entry| &entry.props);
            matches!(props, Some(Props::Space(_)))
        };
        let (spaces, controls): (Vec<usize>, Vec<usize>) =
            drawn(table, children).partition(is_space);
        let (width, height) = Display::drawing_size(wide, !controls.is_empty());
        let controls: Vec<AnyElement> = self.small(|drawing| {
            (controls.into_iter())
                .map(|control| drawing.draw(control))
                .collect()
        });
        let mut display = Display::new(ElementId::Name(name.into()))
            .when(wide, Display::wide)
            .when_some(props.caption.clone(), Display::caption);
        // A place of the view's space, inside the margin, as a place of the whole drawing.
        let across = |x: f32| (DISPLAY_MARGIN + x * (width - 2. * DISPLAY_MARGIN)) / width;
        let up = |y: f32| 1. - (DISPLAY_MARGIN + y * (height - 2. * DISPLAY_MARGIN)) / height;
        let space = (spaces.first()).and_then(|slot| Some((*slot, table.get(*slot)?)));
        let zero = (props.zero).or_else(|| scope_zero(table, &space?.1.children));
        display = display.when_some(zero, |display, zero| display.zero_line(up(zero)));
        if let Some(scale) = &props.scale {
            let lines = scale.x.iter().filter_map(|mark| match mark {
                Mark::Line(x) => Some(across(*x)),
                Mark::Worded(..) => None,
            });
            let worded = scale.x.iter().filter_map(|mark| match mark {
                Mark::Worded(x, word) => Some((across(*x), word.clone())),
                Mark::Line(_) => None,
            });
            display = display
                .grid(lines.collect(), scale.y.iter().map(|y| up(*y)).collect())
                .scale(worded);
        }
        for control in controls {
            display = display.control(control);
        }
        if let Some((slot, entry)) = space
            && let Props::Space(space) = &entry.props
        {
            // One unit is its whole width across and its whole height down, inside the margin.
            let (width, height) = (width - 2. * DISPLAY_MARGIN, height - 2. * DISPLAY_MARGIN);
            let mut space = (**space).clone();
            (space.width, space.height, space.grow) = (Some(width), Some(height), None);
            space.unit = Some(Axes {
                x: Some(width),
                y: Some(height),
            });
            let drawing = self.space(slot, entry.id, &space, &entry.children);
            let placed = (div().absolute())
                .left(px(DISPLAY_MARGIN))
                .top(px(DISPLAY_MARGIN));
            display = display.child(placed.child(drawing));
        }
        // In a card the gap is part of its grid; on a page the row it is in says the room.
        (div().flex_none())
            .when(self.expanded.is_some(), |room| room.mr(px(display::GAP)))
            .child(display)
            .into_any_element()
    }

    /// The cells of a card, two to a column. In a card the first columns show, and the rest
    /// after a hairline while the card is expanded; a page shows them all.
    pub(super) fn controls(&mut self, children: &[usize]) -> AnyElement {
        self.small(|drawing| drawing.cells(children))
    }

    fn cells(&mut self, children: &[usize]) -> AnyElement {
        let table = self.table;
        let cells: Vec<usize> = drawn(table, children).collect();
        let shown = match self.expanded {
            Some(_) => shown_cells(table).min(cells.len()),
            None => cells.len(),
        };
        let (first, rest) = cells.split_at(shown);
        let expanded = self.expanded == Some(true);
        let mut columns = |cells: &[usize]| -> Vec<AnyElement> {
            (cells.chunks(2))
                .map(|column| {
                    let rows =
                        (column.iter()).map(|cell| div().h(px(ROW_HEIGHT)).child(self.cell(*cell)));
                    // A cell narrower than its column stays on the grid, at its left.
                    (div().flex_none().flex().flex_col().items_start())
                        .children(rows)
                        .into_any_element()
                })
                .collect()
        };
        let row = (div().flex().flex_none().h(px(ROW_HEIGHT * 2.))).children(columns(first));
        match expanded && !rest.is_empty() {
            true => row.child(Section::new().children(columns(rest))),
            false => row,
        }
        .into_any_element()
    }

    /// The node in `slot` as a cell: a knob, which draws its own lines, or a control in the
    /// frame of a cell.
    pub(super) fn cell(&mut self, slot: usize) -> AnyElement {
        let table = self.table;
        let Some(entry) = table.get(slot) else {
            return div().into_any_element();
        };
        let (control, label, value) = match &entry.props {
            Props::Space(_) => return self.draw(slot),
            Props::Div(div) if div.layout == Some(Layout::Cell) => return self.draw(slot),
            Props::Cell(CellProps { label, value }) => {
                let control = drawn(table, &entry.children).next();
                (control, label.clone(), value.clone())
            }
            _ => (Some(slot), None, None),
        };
        FittedCell {
            control: control.map(|control| self.draw(control)),
            label: label.map(SharedString::from),
            value: value.map(SharedString::from),
            frame: None,
        }
        .into_any_element()
    }
}

/// A cell as many columns wide as its control needs, which only the layout knows.
struct FittedCell {
    control: Option<AnyElement>,
    label: Option<SharedString>,
    value: Option<SharedString>,
    /// The frame of the cell, with its lines, made once the control is measured.
    frame: Option<AnyElement>,
}

/// The room a control is measured in: as wide and as tall as it is by itself.
fn by_itself() -> Size<AvailableSpace> {
    size(AvailableSpace::MaxContent, AvailableSpace::MinContent)
}

impl IntoElement for FittedCell {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for FittedCell {
    /// The size of the control.
    type RequestLayoutState = Size<Pixels>;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Size<Pixels>) {
        let needs = match &mut self.control {
            Some(control) => control.layout_as_root(by_itself(), window, cx),
            None => Size::default(),
        };
        let span = (f32::from(needs.width) / CELL_WIDTH).ceil().max(1.) as usize;
        let mut frame = Cell::new(Empty)
            .span(span)
            .when_some(self.label.take(), Cell::label)
            .when_some(self.value.take(), Cell::value)
            .into_any_element();
        let layout = frame.request_layout(window, cx);
        self.frame = Some(frame);
        (layout, needs)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        needs: &mut Size<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(frame) = &mut self.frame {
            frame.prepaint(window, cx);
        }
        // In the middle of the room of a knob, as a cell puts its control.
        let origin = bounds.origin
            + point(
                (bounds.size.width - needs.width) / 2.,
                (px(CONTROL_HEIGHT) - needs.height) / 2.,
            );
        if let Some(control) = &mut self.control {
            control.prepaint_as_root(origin, by_itself(), window, cx);
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Size<Pixels>,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        for element in (self.frame.iter_mut()).chain(&mut self.control) {
            element.paint(window, cx);
        }
    }
}
