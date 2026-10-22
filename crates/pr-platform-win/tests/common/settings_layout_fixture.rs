use pr_platform_win::dialogs::settings::builder::{
    ButtonsElement, CheckElement, CheckFlowElement, GroupElement, LayoutElement, RadioElement, RowElement, RowKind,
    SettingsLayoutSurface, SliderElement, TextElement,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub struct GoldenSurface {
    pub dx_mul: i32,
    pub dx_div: i32,
    pub dy_mul: i32,
    pub dy_div: i32,
    pub line_height: i32,
    pub field_height: i32,
    pub avg_char_width: i32,
    pub texts: HashMap<i32, String>,
    pub heights: HashMap<i32, i32>,
    pub hidden: HashSet<i32>,
    pub combo_drop: HashMap<i32, i32>,
    pub calls: Vec<Value>,
}
impl GoldenSurface {
    pub fn from_def(def: &Value) -> Self {
        let sd = &def["surface_def"];
        let map_i = |v: &Value| {
            v.as_object()
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, v)| k.parse::<i32>().ok().zip(v.as_i64().map(|n| n as i32)))
                        .collect()
                })
                .unwrap_or_default()
        };
        let texts = def["texts"]
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| k.parse::<i32>().ok().zip(v.as_str().map(str::to_owned)))
                    .collect()
            })
            .unwrap_or_default();
        let hidden = def["hidden"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_i64).map(|n| n as i32).collect())
            .unwrap_or_default();
        Self {
            dx_mul: sd["dx_mul"].as_i64().unwrap_or(1) as i32,
            dx_div: sd["dx_div"].as_i64().unwrap_or(1) as i32,
            dy_mul: sd["dy_mul"].as_i64().unwrap_or(1) as i32,
            dy_div: sd["dy_div"].as_i64().unwrap_or(1) as i32,
            line_height: sd["line_height"].as_i64().unwrap_or(16) as i32,
            field_height: sd["field_height"].as_i64().unwrap_or(23) as i32,
            avg_char_width: sd["avg_char_width"].as_i64().unwrap_or(7) as i32,
            texts,
            heights: map_i(&def["heights"]),
            hidden,
            combo_drop: map_i(&def["combo_drop"]),
            calls: vec![],
        }
    }
}
impl SettingsLayoutSurface for GoldenSurface {
    fn dx(&self, n: i32) -> i32 {
        n.saturating_mul(self.dx_mul) / self.dx_div.max(1)
    }
    fn dy(&self, n: i32) -> i32 {
        n.saturating_mul(self.dy_mul) / self.dy_div.max(1)
    }
    fn line_height(&self) -> i32 {
        self.line_height
    }
    fn field_height(&self) -> i32 {
        self.field_height
    }
    fn text_of(&self, id: i32) -> String {
        self.texts.get(&id).cloned().unwrap_or_default()
    }
    fn text_width(&self, s: &str) -> i32 {
        (s.encode_utf16().count() as i32).saturating_mul(self.avg_char_width)
    }
    fn text_height(&self, id: i32, w: i32) -> i32 {
        self.heights.get(&id).copied().unwrap_or_else(|| {
            let tw = self.text_width(&self.text_of(id));
            tw.saturating_add(w.max(1) - 1) / w.max(1).max(1) * self.line_height
        })
    }
    fn is_hidden(&self, id: i32) -> bool {
        self.hidden.contains(&id)
    }
    fn combo_drop_height(&self, id: i32) -> i32 {
        self.combo_drop.get(&id).copied().unwrap_or(0)
    }
    fn place(&mut self, id: i32, x: i32, y: i32, w: i32, h: i32) {
        self.calls
            .push(serde_json::json!({"op":"place","id":id,"x":x,"y":y,"width":w,"height":h}));
    }
    fn hide(&mut self, id: i32) {
        self.calls.push(serde_json::json!({"op":"hide","id":id}));
    }
}
pub fn build_element(v: &Value) -> LayoutElement {
    let id = |k: &str| v[k].as_i64().unwrap_or(0) as i32;
    let mut e: LayoutElement = match v["type"].as_str().unwrap_or("") {
        "row" => Box::new(RowElement::new(
            id("label"),
            id("control"),
            if v["kind"] == "combo" {
                RowKind::Combo
            } else {
                RowKind::Edit
            },
            id("width_dlu"),
        )),
        "slider" => Box::new(SliderElement::new(id("label"), id("slider"), id("value"))),
        "text" => Box::new(TextElement::new(id("id"), v["aligned"].as_bool().unwrap_or(false))),
        "buttons" => Box::new(ButtonsElement::new(
            v["ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_i64)
                .map(|n| n as i32)
                .collect(),
            v["aligned"].as_bool().unwrap_or(false),
        )),
        "check" => Box::new(CheckElement::new(id("id"), id("indent_dlu"))),
        "radio" => Box::new(RadioElement::new(
            id("id"),
            v["hint_id"].as_i64().map(|n| n as i32),
            id("indent_dlu"),
        )),
        "check_flow" => Box::new(CheckFlowElement::new(
            v["ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_i64)
                .map(|n| n as i32)
                .collect(),
            id("indent_dlu"),
        )),
        "group" => Box::new(GroupElement::new(
            id("title_id"),
            v["children"].as_array().into_iter().map(build_element).collect(),
        )),
        _ => Box::new(TextElement::new(id("id"), false)),
    };
    if v["visible"] == false {
        e.set_visible(false);
    }
    e
}
