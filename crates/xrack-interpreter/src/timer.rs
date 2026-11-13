use crate::*;
use xrack_runtime::{TimerCommand, TimerConfig, TimerRegistration, TimerTick};

impl Instance {
    fn timer_running(&self, timer: &Element) -> Result<bool> {
        Ok(self.control_value(timer, "running")? == Some(Value::Bool(true)))
    }

    fn timer_config(&self, timer: &Element) -> Result<Option<TimerConfig>> {
        if !self.timer_running(timer)? {
            return Ok(None);
        }
        let interval = match self.binding_value(timer, "interval")? {
            Some(Value::Int(milliseconds)) => milliseconds,
            _ => 1000,
        };
        let repeat = self.binding_value(timer, "repeat")? == Some(Value::Bool(true));
        let immediate = self.binding_value(timer, "triggered-on-start")? == Some(Value::Bool(true));
        Ok(TimerConfig::new(interval, repeat, immediate))
    }

    pub(crate) fn timer_command(&self, target: &str, command: TimerCommand) -> Result<()> {
        let (owner, timer) = self.element_owner(target)?;
        let slot = owner.timer_slot(&timer.key)?;
        match command {
            TimerCommand::Start if owner.timer_running(timer)? => {}
            TimerCommand::Start | TimerCommand::Restart => {
                slot.restart();
                owner.set_control_value(timer, "running", true.into())?;
            }
            TimerCommand::Stop => {
                owner.set_control_value(timer, "running", false.into())?;
            }
        }
        Ok(())
    }

    pub(crate) fn timer_registration(&self, timer: &Element) -> Result<Option<TimerRegistration>> {
        let Some(config) = self.timer_config(timer)? else {
            return Ok(None);
        };
        let key = timer.key.clone();
        let owner = Rc::downgrade(&self.0);
        Ok(Some(self.timer_slot(&timer.key)?.register(
            config,
            move |tick| {
                let Some(owner) = owner.upgrade().map(Instance) else {
                    return false;
                };
                let Some(timer) = owner.definition().root.find_key(&key) else {
                    return false;
                };
                owner.handle_result(owner.trigger_timer(timer, tick))
            },
        )))
    }

    fn trigger_timer(&self, timer: &Element, tick: TimerTick) -> Result<bool> {
        if !self
            .timer_slot(&timer.key)?
            .try_tick_is_current(&tick, || self.timer_config(timer))?
        {
            return Ok(false);
        }
        if tick.completed {
            self.set_control_value(timer, "running", false.into())?;
        }
        self.handler(timer, "triggered", &[])?;
        Ok(true)
    }
}
