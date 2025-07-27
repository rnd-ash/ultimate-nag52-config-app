use std::fmt::Display;

use eframe::egui::{Widget, Key};

pub struct NumberInputWidget<T>
where T: Copy + Display {
    buffer: NumInputBuffer<T>,
    curr_txt: String,
}

impl<T> NumberInputWidget<T> 
where T: Copy + Display {
    pub fn new(raw: T, validator: impl Fn(&str) -> Option<T> + 'static) -> Self {
        let buf = NumInputBuffer::new(raw, validator);
        let s = buf.text.clone();
        Self {
            buffer: buf,
            curr_txt: s,
        }
    }

    pub fn get_val(&self) -> Option<T> {
        self.buffer.get_val()
    }
}

impl<T> Widget for NumberInputWidget<T>
where T: Copy + Display {
    fn ui(mut self, ui: &mut eframe::egui::Ui) -> eframe::egui::Response {
        let mut txt = self.curr_txt.clone();
        let response = ui.text_edit_singleline(&mut txt);
        if response.lost_focus() || ui.input(|i| i.key_pressed(Key::Enter)) {
            // Process new text
            self.buffer.set_text(&txt); 
        } else {
            // Save temp buffer
            self.curr_txt = txt;
        }
        response
    }
}


pub struct NumInputBuffer<T>
where T: Copy + Display {
    text: String,
    value: Option<T>,
    check: Box<dyn Fn(&str) -> Option<T>>
}

impl<T> NumInputBuffer<T>
where T: Copy + Display {
    pub fn new(
        start_value: T,
        validator: impl Fn(&str) -> Option<T> + 'static
    ) -> Self {
        Self {
            text: start_value.to_string(),
            value: Some(start_value),
            check: Box::new(validator)
        }
    }

    pub fn get_val(&self) -> Option<T> {
        self.value
    }

    pub fn is_valid(&self) -> bool {
        self.value.is_some()
    }

    pub fn set_val(&mut self, val: T) {
        self.text = val.to_string();
        self.value = Some(val);
    }

    pub fn set_text(&mut self, txt: &str) {
        self.text = txt.to_string();
        match (self.check)(txt) {
            Some(v) => {
                self.value = Some(v)
            },
            None => {
                self.text = self.value.unwrap().to_string()
            },
        }
    }
}

