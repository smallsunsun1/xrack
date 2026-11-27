use super::{ArrangeContext, Item, ItemAction, ItemEvent, ItemPaint, ItemRef, MeasureContext};
use crate::grid::Placement;
use crate::{LayoutNode, NavigationKey, Point, Rect, Size, TextMeasurer, place_in_direction};
use xrack_runtime::{
    CalendarAction, CalendarHighlight, CalendarHighlightDay, CalendarState, ControlAction, Date,
    LayoutDirection, Node, is_calendar_control,
};

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum CalendarPart {
    Calendar,
    MonthGrid,
    DayOfWeekRow,
    WeekNumberColumn,
}

impl CalendarPart {
    fn dimensions(self) -> (usize, usize) {
        match self {
            Self::Calendar => (7, 8),
            Self::MonthGrid => (7, 6),
            Self::DayOfWeekRow => (7, 1),
            Self::WeekNumberColumn => (1, 6),
        }
    }

    fn from_node(node: &Node) -> Self {
        match node.kind {
            "MonthGrid" => Self::MonthGrid,
            "DayOfWeekRow" => Self::DayOfWeekRow,
            "WeekNumberColumn" => Self::WeekNumberColumn,
            _ => Self::Calendar,
        }
    }

    fn cell(
        self,
        bounds: Rect,
        column: usize,
        row: usize,
        span: usize,
        direction: LayoutDirection,
    ) -> Rect {
        let (columns, rows) = self.dimensions();
        let width = bounds.width / columns as f32;
        let height = bounds.height / rows as f32;
        let rect = Rect::new(
            bounds.x + width * column as f32,
            bounds.y + height * row as f32,
            width * span as f32,
            height,
        );
        place_in_direction(rect, bounds, direction)
    }
}

pub(crate) fn preferred_size(node: &Node, text: &(impl TextMeasurer + ?Sized)) -> Size {
    let part = CalendarPart::from_node(node);
    let mut size = match part {
        CalendarPart::Calendar => Size::new(294.0, 300.0),
        CalendarPart::MonthGrid => Size::new(294.0, 216.0),
        CalendarPart::DayOfWeekRow => Size::new(294.0, 32.0),
        CalendarPart::WeekNumberColumn => Size::new(44.0, 216.0),
    };
    if text.text_scale() > 1.0 {
        let row_height = (text.font_size(node) * 1.8 + 4.0).max(44.0);
        size.height = size.height.max(row_height * part.dimensions().1 as f32);
    }
    size
}

pub(crate) fn month_cell(bounds: Rect, slot: usize, direction: LayoutDirection) -> Rect {
    CalendarPart::MonthGrid.cell(bounds, slot % 7, slot / 7, 1, direction)
}

pub struct CalendarGeometry {
    bounds: Rect,
    part: CalendarPart,
    pub state: CalendarState,
    pub layout_direction: LayoutDirection,
    cells: [Option<Date>; 42],
    week_start: usize,
    today: Option<Date>,
    highlight: CalendarHighlight,
}

#[derive(Clone, Copy)]
pub struct CalendarDay {
    pub date: Date,
    pub slot: usize,
    pub bounds: Rect,
    pub enabled: bool,
    pub selected: bool,
    pub in_month: bool,
    pub today: bool,
    pub highlight: CalendarHighlightDay,
}

#[derive(Clone, Copy)]
pub struct CalendarWeek {
    pub year: i64,
    pub number: i64,
    pub bounds: Rect,
}

impl CalendarGeometry {
    pub fn new(node: &Node, bounds: Rect) -> Self {
        Self::with_layout_direction(node, bounds, node.layout_direction().unwrap_or_default())
    }

    pub fn for_layout(node: &Node, layout: &LayoutNode) -> Self {
        Self::with_layout_direction(node, layout.bounds, layout.layout_direction)
    }

    fn with_layout_direction(node: &Node, bounds: Rect, direction: LayoutDirection) -> Self {
        let state = CalendarState::from_node(node);
        let week_start = node.int("week-start").unwrap_or(1).clamp(1, 7);
        let part = CalendarPart::from_node(node);
        let cells = if part == CalendarPart::DayOfWeekRow {
            [None; 42]
        } else {
            state.cells(week_start)
        };
        let highlight = CalendarHighlight::from_node(node, state.fixed_month);
        Self {
            bounds,
            part,
            state,
            layout_direction: direction,
            cells,
            week_start: week_start as usize - 1,
            today: node.date("today"),
            highlight,
        }
    }

    fn cell(&self, column: usize, row: usize, span: usize) -> Rect {
        self.part
            .cell(self.bounds, column, row, span, self.layout_direction)
    }

    pub fn has_header(&self) -> bool {
        self.part == CalendarPart::Calendar
    }

    pub fn title(&self) -> Rect {
        self.cell(1, 0, 5)
    }

    pub fn navigation(&self) -> [(Rect, Option<CalendarAction>); 2] {
        if !self.has_header() {
            return [(Rect::default(), None); 2];
        }
        [
            (
                self.cell(0, 0, 1),
                self.state.adjacent_month(-1).map(CalendarAction::Browse),
            ),
            (
                self.cell(6, 0, 1),
                self.state.adjacent_month(1).map(CalendarAction::Browse),
            ),
        ]
    }

    pub fn weekdays<'a>(&'a self, node: &'a Node) -> impl Iterator<Item = (Rect, &'a str)> + 'a {
        let labels = node.strings("weekday-labels");
        let count = if matches!(
            self.part,
            CalendarPart::Calendar | CalendarPart::DayOfWeekRow
        ) {
            7
        } else {
            0
        };
        (0..count).map(move |column| {
            let day = (self.week_start + column) % 7;
            let label = labels.get(day).map_or(WEEKDAYS[day], String::as_str);
            (self.cell(column, usize::from(self.has_header()), 1), label)
        })
    }

    pub fn days(&self) -> impl Iterator<Item = CalendarDay> + '_ {
        let count = if matches!(self.part, CalendarPart::Calendar | CalendarPart::MonthGrid) {
            42
        } else {
            0
        };
        self.cells[..count]
            .iter()
            .enumerate()
            .filter_map(|(index, &date)| {
                let date = date?;
                let in_month = self.state.month == date.first_of_month();
                Some(CalendarDay {
                    date,
                    slot: index,
                    bounds: if self.part == CalendarPart::MonthGrid {
                        month_cell(self.bounds, index, self.layout_direction)
                    } else {
                        self.cell(index % 7, 2 + index / 7, 1)
                    },
                    enabled: self.state.allows(date),
                    selected: self.state.selected == date && self.state.allows(date),
                    in_month,
                    highlight: self.highlight.day(date, in_month),
                    today: self.today == Some(date),
                })
            })
    }

    pub fn weeks(&self) -> impl Iterator<Item = CalendarWeek> + '_ {
        let count = if self.part == CalendarPart::WeekNumberColumn {
            42
        } else {
            0
        };
        let thursday = (10 - self.week_start) % 7;
        self.cells[..count]
            .chunks_exact(7)
            .enumerate()
            .filter_map(move |(row, week)| {
                // ISO week numbers follow the Thursday of the displayed row even
                // when the display starts the week on a day other than Monday.
                let date = week[thursday].or_else(|| week.iter().flatten().next().copied())?;
                let (year, number) = date.iso_week()?;
                Some(CalendarWeek {
                    year,
                    number,
                    bounds: self.cell(0, row, 1),
                })
            })
    }

    pub fn cursor(&self) -> Option<Date> {
        if self.has_header() {
            let preferred = self.state.cursor();
            let mut prior = None;
            for day in self.days().filter(|day| day.in_month && day.enabled) {
                if day.date >= preferred {
                    return Some(day.date);
                }
                prior = Some(day.date);
            }
            return prior;
        }
        if let Some(date) = self
            .selection_in_view()
            .filter(|&date| self.state.allows(date))
        {
            return Some(date);
        }
        self.days()
            .find(|day| day.enabled && day.in_month)
            .or_else(|| self.days().find(|day| day.enabled))
            .map(|day| day.date)
    }

    pub(crate) fn navigation_origin(&self) -> Option<Date> {
        // A live exclusion leaves the draft in place. Start directional movement
        // there so the selectable focus fallback isn't skipped a second time.
        self.selection_in_view().or_else(|| self.cursor())
    }

    fn selection_in_view(&self) -> Option<Date> {
        let date = self.state.selected;
        let current_month_only = self.has_header() || self.state.fixed_month;
        (self.cells.contains(&Some(date))
            && (!current_month_only || date.first_of_month() == self.state.month))
            .then_some(date)
    }

    pub fn day_action(&self, date: Date) -> CalendarAction {
        if self.part == CalendarPart::MonthGrid {
            CalendarAction::Activate(date)
        } else {
            CalendarAction::Select(date)
        }
    }

    pub fn action_at(&self, point: Point) -> Option<CalendarAction> {
        for (bounds, action) in self.navigation() {
            if bounds.contains(point) {
                return action;
            }
        }
        self.days()
            .find(|day| day.enabled && day.bounds.contains(point))
            .map(|day| self.day_action(day.date))
    }

    pub fn action_bounds(&self, action: CalendarAction) -> Option<Rect> {
        match action {
            CalendarAction::Select(date) | CalendarAction::Activate(date)
                if action == self.day_action(date) =>
            {
                self.days()
                    .find(|day| day.date == date && day.enabled)
                    .map(|day| day.bounds)
            }
            CalendarAction::Browse(_) => self
                .navigation()
                .into_iter()
                .find_map(|(rect, item)| (item == Some(action)).then_some(rect)),
            _ => None,
        }
    }
}

pub(super) struct Calendar;

impl CalendarGeometry {
    fn navigate(&self, key: NavigationKey, direction: LayoutDirection) -> Option<CalendarAction> {
        let calendar = &self.state;
        let day = |delta| {
            let date = self
                .navigation_origin()?
                .add_days(delta)
                .unwrap_or(if delta < 0 { Date::MIN } else { Date::MAX })
                .clamp(calendar.minimum, calendar.maximum);
            calendar.selectable_from(date, delta > 0)
        };
        let month = |offset| {
            if self.has_header() {
                calendar.adjacent_month(offset)
            } else {
                calendar.month.add_months(offset)
            }
            .unwrap_or(calendar.month)
        };
        let right_step = if direction == LayoutDirection::RightToLeft {
            -1
        } else {
            1
        };
        Some(match key {
            NavigationKey::Left => CalendarAction::Select(day(-right_step)?),
            NavigationKey::Right => CalendarAction::Select(day(right_step)?),
            NavigationKey::Up => CalendarAction::Select(day(-7)?),
            NavigationKey::Down => CalendarAction::Select(day(7)?),
            NavigationKey::Home | NavigationKey::End => {
                let date = if key == NavigationKey::Home {
                    calendar.month.max(calendar.minimum)
                } else {
                    calendar.month.last_of_month().min(calendar.maximum)
                };
                let date = calendar.selectable_from(date, key == NavigationKey::Home)?;
                if date.first_of_month() != calendar.month {
                    return None;
                }
                CalendarAction::Select(date)
            }
            NavigationKey::PageUp | NavigationKey::PageDown if calendar.fixed_month => return None,
            NavigationKey::PageUp => CalendarAction::Browse(month(-1)),
            NavigationKey::PageDown => CalendarAction::Browse(month(1)),
        })
    }
}

impl Item for Calendar {
    fn focusable(&self, node: &Node) -> bool {
        is_calendar_control(node.kind)
    }

    fn event(&self, item: ItemRef<'_, '_>, event: ItemEvent) -> Option<ItemAction> {
        if !is_calendar_control(item.node.kind) {
            return None;
        }
        let geometry = CalendarGeometry::for_layout(item.node, item.layout);
        let action = match event {
            ItemEvent::HitTest(point) => {
                match geometry.action_at(item.layout.local_to_layout(point)) {
                    Some(action) => action,
                    None => return Some(ItemAction::BlockHit),
                }
            }
            ItemEvent::Navigate(key) => geometry.navigate(key, item.layout.layout_direction)?,
            ItemEvent::Activate => {
                let date = geometry.cursor()?;
                if !geometry.state.allows(date) {
                    return None;
                }
                geometry.day_action(date)
            }
            _ => return None,
        };
        Some(ItemAction::Control(ControlAction::Calendar(action)))
    }

    fn arrange(&self, cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        if cx.node.kind != "MonthGrid" {
            return None;
        }
        cx.node.calendar_cells()?;
        Some(
            cx.node
                .children
                .iter()
                .map(|child| {
                    Placement::plain(if child.kind == "CalendarCells" {
                        cx.bounds
                    } else {
                        cx.layout.absolute_child(child, cx.bounds)
                    })
                })
                .collect(),
        )
    }

    fn paint(&self, node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::Calendar {
            framed: is_calendar_control(node.kind),
        }
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let size = crate::calendar::preferred_size(cx.node, cx.layout);
        cx.intrinsic(size)
    }
}
