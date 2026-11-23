use puzzle_ui::Puzzle3D;
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct Game {
    x: i8,
    z: i8,
    amber: bool,
    blue: bool,
    finished: bool,
}

impl Game {
    fn move_player(&mut self, direction: &str) -> &'static str {
        let (dx, dz) = match direction {
            "north" => (0, -1),
            "south" => (0, 1),
            "west" => (-1, 0),
            "east" => (1, 0),
            _ => return "无法移动",
        };
        let (x, z) = (self.x + dx, self.z + dz);
        if self.finished {
            return "已到达出口";
        }
        if !(-2..=2).contains(&x) || !(-2..=2).contains(&z) || (z < 0 && x != 0) {
            return "墙挡住了路";
        }
        if z == -2 && !(self.amber && self.blue) {
            return "门还没有打开";
        }
        (self.x, self.z) = (x, z);
        self.finished = z == -2;
        if self.finished {
            "解谜完成 · 已到达出口"
        } else {
            "靠近机关后点击或按 E"
        }
    }

    fn activate(&mut self, id: &str) -> &'static str {
        let x = match id {
            "amber" => -1,
            "blue" => 1,
            _ => return "点击琥珀色或蓝色机关",
        };
        if (self.x - x).abs() + self.z.abs() > 1 {
            return "离机关太远了";
        }
        if id == "amber" {
            self.amber = true;
            "琥珀机关已激活"
        } else if self.amber {
            self.blue = true;
            "蓝色机关已激活 · 门打开了"
        } else {
            "先激活琥珀机关"
        }
    }

    fn publish(&self, ui: &mut Puzzle3D, status: &str) {
        ui.set_amber_on(self.amber);
        ui.set_blue_on(self.blue);
        ui.set_player_x(f64::from(self.x));
        ui.set_player_z(f64::from(self.z));
        ui.set_finished(self.finished);
        ui.set_status(status.into());
    }
}

fn run() -> Result<(), String> {
    #[cfg(feature = "live-ui")]
    let mut ui = Puzzle3D::try_new().map_err(|error| error.to_string())?;
    #[cfg(not(feature = "live-ui"))]
    let mut ui = Puzzle3D::new();
    let game = Rc::new(RefCell::new(Game {
        z: 1,
        ..Game::default()
    }));
    ui.set_playing(true);
    game.borrow().publish(&mut ui, "用方向键、WASD 或按钮移动");
    let moving = game.clone();
    let weak = ui.as_weak();
    ui.on_move_player(move |direction| {
        let mut game = moving.borrow_mut();
        let status = game.move_player(&direction);
        if let Some(mut ui) = weak.upgrade() {
            game.publish(&mut ui, status);
        }
    });
    let weak = ui.as_weak();
    ui.on_activate_object(move |id| {
        let mut game = game.borrow_mut();
        let status = game.activate(&id);
        if let Some(mut ui) = weak.upgrade() {
            game.publish(&mut ui, status);
        }
    });
    xrack_platform_winit::run(ui)
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
