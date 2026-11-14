use crate::{Element, Error, Instance, Result, repeat::Model};
use xrack_runtime::{
    ListModel, Node, RangeAction, ScrollBarRange, SliderInterval, SliderRange, SpinAction, Value,
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

    fn range_with_live_bounds(&self, element: &Element, range: SliderRange) -> Result<SliderRange> {
        let bound = |name, default| match self.binding_value(element, name)? {
            Some(Value::Float(value)) => Ok(value),
            _ => Ok::<_, Error>(default),
        };
        Ok(range.with_bounds(bound("minimum", 0.0)?, bound("maximum", 100.0)?))
    }

    fn row_numeric_input_available(
        &self,
        element: &Element,
        owner: &Self,
        view: Option<&Element>,
        model: &Model,
    ) -> Result<bool> {
        model.contents_stamp();
        if let Some(tree) = view.filter(|view| view.tree.is_some()) {
            if !owner.editor_view_available(tree)? {
                return Ok(false);
            }
            owner.tree_projection(tree)?;
        }
        self.input_element_available(element, false)
    }

    pub(crate) fn range_action(
        &self,
        element: &Element,
        node: &Node,
        action: RangeAction,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let range = SliderRange::from_node(node);
        if let Some(field) = element.row_links.get("value") {
            let Some(value) = range.apply(action) else {
                return Ok(false);
            };
            return self.prepare_row_field_write(
                field,
                node.properties.get("value"),
                value.into(),
                || Ok(true),
                |owner, view, model, _, history| {
                    let prepared = xrack_runtime::try_prepare_range_view(
                        node,
                        action,
                        || self.row_numeric_input_available(element, owner, view, model),
                        |name| self.binding_value(element, name),
                        || input.try_prepare(|| Ok(true), || history().map(Some)),
                    )?;
                    Ok(prepared.flatten())
                },
            );
        }
        let Some(current) = xrack_runtime::try_prepare_range_view(
            node,
            action,
            || self.element_allows_routed_input(element, input),
            |name| self.binding_value(element, name),
            || self.range_value(element, "value"),
        )?
        else {
            return Ok(false);
        };
        if !input.is_current() {
            return Ok(false);
        }
        let range = range.with_value(current);
        action.try_deliver(
            range.apply(action),
            |value| {
                if !self.update_control(element, node, "value", value.into())? {
                    return Ok(false);
                }
                self.handler(element, "changed", &[value.into()])?;
                Ok(true)
            },
            || {
                let value = self
                    .range_with_live_bounds(element, range)?
                    .with_value(self.range_value(element, "value")?)
                    .value();
                self.handler(element, "released", &[value.into()])
                    .map(|_| ())
            },
        )
    }

    pub(crate) fn scroll_bar_action(
        &self,
        element: &Element,
        node: &Node,
        action: RangeAction,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let Some(position) = xrack_runtime::try_prepare_range_view(
            node,
            action,
            || self.element_allows_routed_input(element, input),
            |name| self.binding_value(element, name),
            || self.range_value(element, "position"),
        )?
        else {
            return Ok(false);
        };
        if !input.is_current() {
            return Ok(false);
        }
        let Some(value) = ScrollBarRange::from_node(node)
            .with_position(position)
            .apply(action)
        else {
            return Ok(false);
        };
        let value = Value::Float(value);
        if !self.update_control(element, node, "position", value.clone())? {
            return Ok(false);
        }
        self.handler(element, "changed", &[value])?;
        Ok(true)
    }

    pub(crate) fn interval_action(
        &self,
        element: &Element,
        node: &Node,
        handle: usize,
        action: RangeAction,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let values = || {
            Ok([
                self.range_value(element, "lower-value")?,
                self.range_value(element, "upper-value")?,
            ])
        };
        let Some([lower, upper]) = xrack_runtime::try_prepare_range_view(
            node,
            action,
            || self.element_allows_routed_input(element, input),
            |name| self.binding_value(element, name),
            &values,
        )?
        else {
            return Ok(false);
        };
        if !input.is_current() {
            return Ok(false);
        }
        let interval = SliderInterval::from_values(node, lower, upper);
        let dispatch = |event| {
            let range = self.range_with_live_bounds(element, SliderRange::from_node(node))?;
            let [lower, upper] = values()?;
            let values = SliderInterval::from_range(range, lower, upper)
                .values()
                .map(Value::Float);
            self.handler(element, event, &values)
        };
        action.try_deliver(
            interval.changed_values(handle, action, [lower, upper]),
            |[next_lower, next_upper]| {
                if let Some(value) = next_lower {
                    self.update_control(element, node, "lower-value", value.into())?;
                }
                if let Some(value) = next_upper {
                    self.update_control(element, node, "upper-value", value.into())?;
                }
                dispatch("changed")?;
                Ok(true)
            },
            || dispatch("released").map(|_| ()),
        )
    }

    pub(crate) fn spin_action(
        &self,
        element: &Element,
        node: &Node,
        action: SpinAction,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let value = if node.kind == "DoubleSpinBox" {
            xrack_runtime::decimal_spin_value_after(node, action).map(Value::Float)
        } else {
            xrack_runtime::spin_value_after(node, action).map(Value::Int)
        };
        let Some(value) = value else { return Ok(false) };
        let current = || {
            self.control_value(element, "value")?
                .map_or_else(|| self.default_control_value(element, "value"), Ok)
        };
        if let Some(field) = element.row_links.get("value") {
            return self.prepare_row_field_write(
                field,
                node.properties.get("value"),
                value,
                || Ok(true),
                |owner, view, model, _, history| {
                    xrack_runtime::try_prepare_spin_view(
                        node,
                        action,
                        || self.row_numeric_input_available(element, owner, view, model),
                        current,
                        |name| self.binding_value(element, name),
                        || input.try_prepare(|| Ok(true), || history().map(Some)),
                    )
                },
            );
        }
        let current = xrack_runtime::try_spin_view_is_current(
            node,
            action,
            || self.element_allows_routed_input(element, input),
            current,
            |name| self.binding_value(element, name),
        )?;
        if !current
            || !input.is_current()
            || !self.update_control(element, node, "value", value.clone())?
        {
            return Ok(false);
        }
        self.handler(element, "edited", &[value])?;
        Ok(true)
    }
}
