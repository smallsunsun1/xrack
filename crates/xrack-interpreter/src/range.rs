use crate::{Element, Error, Instance, Result};
use xrack_runtime::{
    Node, RangeAction, SliderInterval, SliderRange, SpinAction, SpinDecimal, Value,
};

impl Instance {
    fn range_value(&self, element: &Element, name: &str) -> Result<f64> {
        match self
            .control_value(element, name)?
            .map_or_else(|| self.default_control_value(element, name), Ok)?
        {
            Value::Float(value) => Ok(value),
            _ => Err(Error::Evaluation(format!(
                "invalid numeric control property {name}"
            ))),
        }
    }

    pub(crate) fn range_action(
        &self,
        element: &Element,
        node: &Node,
        action: RangeAction,
    ) -> Result<bool> {
        let range = SliderRange::from_node(node);
        if element.row_links.contains_key("value") {
            let Some(value) = range.apply(action) else {
                return Ok(false);
            };
            return self.update_control(element, node, "value", value.into());
        }
        let range = range.with_value(self.range_value(element, "value")?);
        if let Some(value) = range.apply(action) {
            if !self.update_control(element, node, "value", value.into())? {
                return Ok(false);
            }
            self.handler(element, "changed", &[value.into()])?;
        } else if !action.is_release() {
            return Ok(false);
        }
        if action.is_release() {
            let value = range
                .with_value(self.range_value(element, "value")?)
                .value();
            self.handler(element, "released", &[value.into()])?;
        }
        Ok(true)
    }

    pub(crate) fn interval_action(
        &self,
        element: &Element,
        node: &Node,
        handle: usize,
        action: RangeAction,
    ) -> Result<bool> {
        let values = || {
            Ok([
                self.range_value(element, "lower-value")?,
                self.range_value(element, "upper-value")?,
            ])
        };
        let [lower, upper] = values()?;
        let interval = SliderInterval::from_values(node, lower, upper);
        let dispatch = |event| {
            let [lower, upper] = values()?;
            let values = SliderInterval::from_values(node, lower, upper)
                .values()
                .map(Value::Float);
            self.handler(element, event, &values)
        };
        if let Some([next_lower, next_upper]) = interval.apply(handle, action) {
            if next_lower != lower {
                self.update_control(element, node, "lower-value", next_lower.into())?;
            }
            if next_upper != upper {
                self.update_control(element, node, "upper-value", next_upper.into())?;
            }
            dispatch("changed")?;
        } else if !action.is_release() {
            return Ok(false);
        }
        if action.is_release() {
            dispatch("released")?;
        }
        Ok(true)
    }

    pub(crate) fn spin_action(
        &self,
        element: &Element,
        node: &Node,
        action: SpinAction,
    ) -> Result<bool> {
        let value = if node.kind == "DoubleSpinBox" {
            if let SpinAction::CommitDecimalIfCurrent { original, .. } = action
                && original != SpinDecimal::new(self.range_value(element, "value")?)
            {
                return Ok(false);
            }
            xrack_runtime::decimal_spin_value_after(node, action).map(Value::Float)
        } else {
            xrack_runtime::spin_value_after(node, action).map(Value::Int)
        };
        let Some(value) = value else { return Ok(false) };
        if !self.update_control(element, node, "value", value.clone())? {
            return Ok(false);
        }
        self.handler(element, "edited", &[value])?;
        Ok(true)
    }
}
