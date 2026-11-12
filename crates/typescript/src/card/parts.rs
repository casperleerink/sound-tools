//! The parts of a device card a view names, drawn with what draws the built-in cards: the
//! display at its left, and its cells in two rows, of which the first columns show and the rest
//! wait behind the expand icon of the card.

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ElementId, Empty, GlobalElementId, InspectorElementId,
    LayoutId, Pixels, SharedString, Size, Window, div, point, prelude::*, px, size,
};
use sound_ui::components::cell::{CELL_WIDTH, CONTROL_HEIGHT, Cell, ROW_HEIGHT};
use sound_ui::components::device_card::Section;
use sound_ui::components::display::{self, Display};

use crate::tree::{Axes, CellProps, DisplayProps, NodeTable, Props};

use super::drawing::Drawing;

/// The columns of cells a card shows before its expand icon: beside a display, as the built-in
/// cards, and with no display.
const COLUMNS_BESIDE_DISPLAY: usize = 2;
const COLUMNS_ALONE: usize = 4;

/// The children of a node that draw.
fn drawn<'a>(table: &'a NodeTable, children: &'a [usize]) -> impl Iterator<Item = usize> + 'a {
    (children.iter().copied()).filter(|child| {
        !matches!(
            table.get(*child).map(|child| &child.props),
            Some(Props::Marker) | None
        )
    })
}

/// How many cells of each `Controls` a card shows before its expand icon.
fn shown_cells(table: &NodeTable) -> usize {
    let display = (table.entries()).any(|entry| matches!(entry.props, Props::Display(_)));
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
        let mut display = Display::new(ElementId::Name(name.into()))
            .when(wide, Display::wide)
            .when_some(props.caption.clone(), Display::caption);
        for control in controls {
            display = display.control(self.draw(control));
        }
        let space = (spaces.first()).and_then(|slot| Some((*slot, table.get(*slot)?)));
        if let Some((slot, entry)) = space
            && let Props::Space(space) = &entry.props
        {
            // One unit is its whole width across and its whole height down.
            let mut space = (**space).clone();
            (space.width, space.height, space.grow) = (Some(width), Some(height), None);
            space.unit = Some(Axes {
                x: Some(width),
                y: Some(height),
            });
            let drawing = self.space(slot, entry.id, &space, &entry.children);
            display = display.child(div().absolute().left_0().top_0().child(drawing));
        }
        (div().flex_none().mr(px(display::GAP)).child(display)).into_any_element()
    }

    /// The cells of a card, two to a column. In a card the first columns show, and the rest
    /// after a hairline while the card is expanded; a page shows them all.
    pub(super) fn controls(&mut self, children: &[usize]) -> AnyElement {
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
