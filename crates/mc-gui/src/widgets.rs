//! `GuiButton`, `GuiOptionButton`, `GuiOptionSlider` and the language button.

use crate::batch::GuiBatch;
use crate::settings::Opt;
use crate::Ui;

pub const WIDGETS: &str = "textures/gui/widgets.png";

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Plain,
    /// Cycles a setting on click.
    OptionButton(Opt),
    /// Drags a float setting; `value` is normalized 0–1.
    Slider { opt: Opt, value: f32, dragging: bool },
    /// `GuiButtonLanguage`: 20×20 globe icon.
    Language,
}

#[derive(Clone, Debug)]
pub struct Button {
    pub id: i32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub kind: Kind,
}

impl Button {
    /// `new GuiButton(id, x, y, text)`: 200×20.
    pub fn new(id: i32, x: i32, y: i32, label: impl Into<String>) -> Self {
        Self::sized(id, x, y, 200, 20, label)
    }

    pub fn sized(id: i32, x: i32, y: i32, w: i32, h: i32, label: impl Into<String>) -> Self {
        Self { id, x, y, w, h, label: label.into(), enabled: true, visible: true, kind: Kind::Plain }
    }

    /// `GuiOptionButton` / `GuiOptionSlider`: 150×20 bound to a setting.
    pub fn option(id: i32, x: i32, y: i32, opt: Opt, ui: &Ui) -> Self {
        let kind = match opt.range() {
            Some(_) => Kind::Slider { opt, value: opt.normalize(ui.settings.float(opt)), dragging: false },
            None => Kind::OptionButton(opt),
        };
        Self { kind, ..Self::sized(id, x, y, 150, 20, ui.settings.label(opt, &ui.lang)) }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn hit(&self, mx: i32, my: i32) -> bool {
        mx >= self.x && my >= self.y && mx < self.x + self.w && my < self.y + self.h
    }

    /// `mousePressed` + the slider's grab.
    pub fn press(&mut self, ui: &mut Ui, mx: i32, my: i32) -> bool {
        if !(self.enabled && self.visible && self.hit(mx, my)) {
            return false;
        }
        match &mut self.kind {
            Kind::Slider { dragging, .. } => {
                *dragging = true;
                self.drag(ui, mx);
            }
            Kind::OptionButton(opt) => {
                let opt = *opt;
                ui.settings.cycle(opt);
                self.label = ui.settings.label(opt, &ui.lang);
            }
            _ => {}
        }
        true
    }

    pub fn release(&mut self) {
        if let Kind::Slider { dragging, .. } = &mut self.kind {
            *dragging = false;
        }
    }

    /// `GuiOptionSlider.mouseDragged`
    fn drag(&mut self, ui: &mut Ui, mx: i32) {
        if let Kind::Slider { opt, value, dragging: true } = &mut self.kind {
            let v = ((mx - (self.x + 4)) as f32 / (self.w - 8) as f32).clamp(0.0, 1.0);
            let f = opt.denormalize(v);
            ui.settings.set_float(*opt, f);
            *value = opt.normalize(f);
            self.label = ui.settings.label(*opt, &ui.lang);
        }
    }

    /// `getHoverState`: 0 disabled, 1 normal, 2 hovered. Sliders always draw as 0.
    fn hover_state(&self, hovered: bool) -> i32 {
        if matches!(self.kind, Kind::Slider { .. }) || !self.enabled {
            0
        } else if hovered {
            2
        } else {
            1
        }
    }

    /// `drawButton`
    pub fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, mx: i32, my: i32) {
        if !self.visible {
            return;
        }
        let hovered = self.hit(mx, my);
        b.color = [1.0; 4];
        if self.kind == Kind::Language {
            // GuiButtonLanguage: icon at (0, 106), +20 when hovered.
            let v = if hovered { 106 + 20 } else { 106 };
            b.textured_rect(WIDGETS, self.x, self.y, 0, v, self.w, self.h);
            return;
        }
        let i = self.hover_state(hovered);
        b.textured_rect(WIDGETS, self.x, self.y, 0, 46 + i * 20, self.w / 2, self.h);
        b.textured_rect(WIDGETS, self.x + self.w / 2, self.y, 200 - self.w / 2, 46 + i * 20, self.w / 2, self.h);
        // mouseDragged runs from drawButton in vanilla.
        self.drag(ui, mx);
        if let Kind::Slider { value, .. } = self.kind {
            let kx = self.x + (value * (self.w - 8) as f32) as i32;
            b.textured_rect(WIDGETS, kx, self.y, 0, 66, 4, 20);
            b.textured_rect(WIDGETS, kx + 4, self.y, 196, 66, 4, 20);
        }
        let color = if !self.enabled {
            10526880
        } else if hovered {
            16777120
        } else {
            14737632
        };
        ui.font.draw_centered(b, &self.label, self.x + self.w / 2, self.y + (self.h - 8) / 2, color);
    }
}
