use std::{cell::RefCell, collections::BTreeMap};
use xrack_runtime::{
    Color, CustomItem, CustomItemChange, CustomItemContext, CustomItemData, CustomItemDrawing,
    CustomItemEvent, CustomItemId, CustomItemRect, CustomItemUpdate, EventResult, KeyPhase,
};

#[derive(Default)]
pub struct Meter {
    levels: RefCell<BTreeMap<CustomItemId, f32>>,
}

impl CustomItem for Meter {
    fn measure(&self, _: CustomItemData<'_>, _: [f32; 2]) -> [f32; 2] {
        [100.0, 36.0]
    }

    fn baseline(&self, _: CustomItemData<'_>, size: [f32; 2]) -> f32 {
        size[1] * 0.7
    }

    fn contains(&self, context: CustomItemContext<'_>, [x, y]: [f32; 2]) -> bool {
        context.placement.bounds.contains([x, y])
    }

    fn paint(&self, context: CustomItemContext<'_>, out: &mut Vec<CustomItemDrawing>) {
        let bounds = context.placement.bounds;
        let levels = self.levels.borrow();
        let level = context
            .state
            .id
            .and_then(|id| levels.get(&id))
            .copied()
            .unwrap_or(0.25);
        out.push(CustomItemDrawing::Fill {
            bounds,
            color: Color(0x30485fff),
        });
        out.push(CustomItemDrawing::Fill {
            bounds: CustomItemRect::new(0.0, 0.0, bounds.width * level, bounds.height),
            color: Color(0x44cc9aff),
        });
    }

    fn event(&self, context: CustomItemContext<'_>, event: CustomItemEvent<'_>) -> EventResult {
        let Some(id) = context.state.id else {
            return EventResult::Reject;
        };
        let mut values = self.levels.borrow_mut();
        let level = values.entry(id).or_insert(0.25);
        match event {
            CustomItemEvent::Activate => {
                *level = if *level >= 1.0 {
                    0.0
                } else {
                    (*level + 0.25).min(1.0)
                };
            }
            CustomItemEvent::Key(KeyPhase::Pressed, event) if event.key == "ArrowLeft" => {
                *level = (*level - 0.1).max(0.0);
            }
            CustomItemEvent::Key(KeyPhase::Pressed, event) if event.key == "ArrowRight" => {
                *level = (*level + 0.1).min(1.0);
            }
            _ => return EventResult::Reject,
        }
        context.request_update(CustomItemUpdate::Paint);
        EventResult::Accept
    }

    fn changed(&self, context: CustomItemContext<'_>, change: CustomItemChange) {
        if change == CustomItemChange::Unmounted
            && let Some(id) = context.state.id
        {
            self.levels.borrow_mut().remove(&id);
        }
    }
}
