use std::{fmt::Display, sync::Arc};

use eframe::{egui::*, epaint::PathShape};
use plotters_backend::{DrawingBackend, DrawingErrorKind, BackendCoord};

mod color;
pub use color::*;

#[derive(Debug, Clone, Copy)]
pub enum DrawingError {
    None,
}

impl Display for DrawingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("None")
    }
}

impl std::error::Error for DrawingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

pub struct EguiPlotBackend {
    painter: Painter,
    style: Arc<Style>,
    clip_rect: Rect
}

impl EguiPlotBackend {
    pub fn new(painter: Painter, style: Arc<Style>) -> Self {
        Self {
            clip_rect: painter.clip_rect(),
            painter,
            style
        }
    }

    fn backend_coord_to_pos2(&self, b: BackendCoord) -> Pos2 {
        Pos2::new(b.0 as f32 + self.clip_rect.left_top().x, b.1 as f32 + self.clip_rect.left_top().y)
    }
}

impl DrawingBackend for EguiPlotBackend {
    type ErrorType = DrawingError;

    fn get_size(&self) -> (u32, u32) {
        (self.clip_rect.width() as u32, self.clip_rect.height() as u32)
    }

    fn ensure_prepared(
        &mut self,
    ) -> Result<(), plotters_backend::DrawingErrorKind<Self::ErrorType>> {
        Ok(())
    }

    fn present(&mut self) -> Result<(), plotters_backend::DrawingErrorKind<Self::ErrorType>> {
        Ok(())
    }

    fn draw_pixel(
        &mut self,
        point: plotters_backend::BackendCoord,
        color: plotters_backend::BackendColor,
    ) -> Result<(), plotters_backend::DrawingErrorKind<Self::ErrorType>> {
        let rect = Rect::from_points(
            &[self.backend_coord_to_pos2(point), self.backend_coord_to_pos2(point)]
        );
        let c = into_egui_color(color);
        self.painter.rect_filled(rect, 0.0, c);
        Ok(())
    }

    fn draw_line<S: plotters_backend::BackendStyle>(
        &mut self,
        from: plotters_backend::BackendCoord,
        to: plotters_backend::BackendCoord,
        style: &S,
    ) -> Result<(), DrawingErrorKind<Self::ErrorType>> {
        self.painter.line_segment([
            self.backend_coord_to_pos2(from), 
            self.backend_coord_to_pos2(to)
            ],
            Stroke::new(
                style.stroke_width() as f32,
                into_egui_color(style.color())
            ),
        );
        Ok(())
    }

    fn draw_path<S: plotters_backend::BackendStyle, I: IntoIterator<Item = plotters_backend::BackendCoord>>(
            &mut self,
            path: I,
            style: &S,
        ) -> Result<(), DrawingErrorKind<Self::ErrorType>> {
            let mut p : Vec<Pos2> = Vec::new();
            let c = style.color();
            let tr = self.painter.clip_rect().left_top();
            for point in path.into_iter() {
                p.push(Pos2::new(point.0 as f32 + tr.x, point.1 as f32 + tr.y))
            }
            let s = Shape::Path(
                PathShape::line(p, 
                    Stroke::new(
                        style.stroke_width() as f32,
                        Color32::from_rgba_unmultiplied(c.rgb.0, c.rgb.1, c.rgb.2, (c.alpha * 255.0) as u8),
                    ))
            );
            self.painter.add(s);
            Ok(())
    }

    fn draw_circle<S: plotters_backend::BackendStyle>(
            &mut self,
            center: plotters_backend::BackendCoord,
            radius: u32,
            style: &S,
            fill: bool,
        ) -> Result<(), DrawingErrorKind<Self::ErrorType>> {
            let r = self.painter.clip_rect();
            let epos = Pos2::new(center.0 as f32 + r.left_top().x, center.1 as f32 + r.left_top().y);
            let c = into_egui_color(style.color());
            let stroke = Stroke::new(style.stroke_width() as f32, c);
            let fill_c = if fill {
                c
            } else {
                Color32::from_rgba_unmultiplied(0, 0, 0, 0)
            };
            self.painter.circle(epos, radius as f32, fill_c, stroke);
            Ok(())
        }
    
    fn draw_rect<S: plotters_backend::BackendStyle>(
            &mut self,
            upper_left: plotters_backend::BackendCoord,
            bottom_right: plotters_backend::BackendCoord,
            style: &S,
            fill: bool,
        ) -> Result<(), DrawingErrorKind<Self::ErrorType>> {
            let c = style.color();
            let r = self.painter.clip_rect();
            let rect = Rect::from_points(
                &[
                     Pos2::new(upper_left.0 as f32 + r.left_top().x, upper_left.1 as f32 + r.left_top().y),
                     Pos2::new(bottom_right.0 as f32 + r.left_top().x, bottom_right.1 as f32 + r.left_top().y)
                 ]
             );
            let egui_c = Color32::from_rgba_unmultiplied(c.rgb.0, c.rgb.1, c.rgb.2, (c.alpha * 255.0) as u8);
            let stroke = Stroke::new(
                style.stroke_width() as f32,
                egui_c,
            );

            //if fill {
            //    self.painter.rect(rect, 0.0, egui_c, stroke)
            //} else {
            //    self.painter.rect_stroke(rect, 0.0, stroke)
            //}

            Ok(())
        }
    
    fn draw_text<TStyle: plotters_backend::BackendTextStyle>(
            &mut self,
            text: &str,
            _style: &TStyle,
            pos: plotters_backend::BackendCoord,
        ) -> Result<(), DrawingErrorKind<Self::ErrorType>> {
            let fid = TextStyle::Monospace.resolve(&self.style);
            self.painter.text(
                self.backend_coord_to_pos2(pos), 
                Align2::CENTER_TOP, 
                text, 
                fid, 
                self.style.visuals.text_color()
            );
            Ok(())
    }

}
