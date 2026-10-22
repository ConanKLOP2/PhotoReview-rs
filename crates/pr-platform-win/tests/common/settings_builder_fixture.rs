use pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder;
use serde_json::{Value, json};

/// Runs the builder operation vocabulary used by the W8-19c oracle.
pub fn run_ops(builder: &mut SettingsPageBuilder, ops: &[Value]) -> Vec<Value> {
    let mut output = Vec::new();
    for op in ops {
        let Some(name) = op.get("op").and_then(Value::as_str) else {
            continue;
        };
        let errors_before = builder.errors().len();
        let result = match name {
            "check" => json!(builder.check(
                provider(op, "text").unwrap_or_else(|| value_text(&Value::Null)),
                number(op, "indent_dlu", 0)
            )),
            "combo" => {
                let items = op
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|values| values.iter().map(value_text).collect());
                let row = builder.combo(
                    provider(op, "label").unwrap_or_else(|| value_text(&Value::Null)),
                    items,
                    number(op, "width_dlu", 0),
                );
                json!({"label": row.label, "control": row.control})
            }
            "edit" => {
                let row = builder.edit(
                    provider(op, "label").unwrap_or_else(|| value_text(&Value::Null)),
                    number(op, "width_dlu", 60),
                );
                json!({"label": row.label, "control": row.control})
            }
            "slider" => {
                let row = builder.slider(
                    provider(op, "label").unwrap_or_else(|| value_text(&Value::Null)),
                    number(op, "min", 0),
                    number(op, "max", 0),
                );
                json!({"label": row.label, "slider": row.slider, "value": row.value})
            }
            "note" => json!(builder.note(provider(op, "text"), boolean(op, "aligned", true))),
            "text" => json!(builder.text(provider(op, "text"))),
            "buttons" => {
                let values = op.get("texts").and_then(Value::as_array).cloned().unwrap_or_default();
                let providers: Vec<_> = values.iter().map(value_text).collect();
                json!(builder.buttons(boolean(op, "aligned", false), &providers))
            }
            "radio" => {
                let values = op.get("texts").and_then(Value::as_array).cloned().unwrap_or_default();
                let providers: Vec<_> = values.iter().map(value_text).collect();
                let hints = op.get("hints").and_then(Value::as_array).map(|items| {
                    items
                        .iter()
                        .map(|value| if value.is_null() { None } else { Some(value_text(value)) })
                        .collect::<Vec<_>>()
                });
                json!(builder.radio(&providers, hints.as_deref(), number(op, "indent_dlu", 0)))
            }
            "check_flow" => {
                let values = op.get("texts").and_then(Value::as_array).cloned().unwrap_or_default();
                let providers: Vec<_> = values.iter().map(value_text).collect();
                json!(builder.check_flow(&providers, number(op, "indent_dlu", 0)))
            }
            "warning" => json!(builder.warning(provider(op, "text"))),
            "key_grid" => {
                let values = op.get("cells").and_then(Value::as_array).cloned().unwrap_or_default();
                let specs: Vec<_> = values
                    .iter()
                    .map(
                        |cell| pr_platform_win::dialogs::settings::builder::layout::SettingsKeyCellSpec {
                            label: value_text(cell.get("label").unwrap_or(&Value::Null)),
                            clearable: cell.get("clearable").and_then(Value::as_bool).unwrap_or(false),
                        },
                    )
                    .collect();
                json!(
                    builder
                        .key_grid(&specs)
                        .iter()
                        .map(|cell| json!({"label":cell.label,"box":cell.box_id,"clear":cell.clear}))
                        .collect::<Vec<_>>()
                )
            }
            "group" => {
                let nested = op.get("ops").and_then(Value::as_array).cloned().unwrap_or_default();
                let title = value_text(op.get("title").unwrap_or(&Value::Null));
                let id = builder.group(title, |builder| {
                    output.extend(run_ops(builder, &nested));
                });
                json!(id)
            }
            "repeat" => {
                let count = number(op, "count", 0).max(0) as usize;
                if let Some(inner) = op.get("op_value") {
                    for _ in 0..count {
                        output.extend(run_ops(builder, std::slice::from_ref(inner)));
                    }
                }
                continue;
            }
            _ => continue,
        };
        if builder.errors().len() > errors_before {
            let error = builder.errors().last().unwrap();
            output.push(json!({"threw": if matches!(error, pr_platform_win::dialogs::settings::builder::inputs::SettingsBuilderError::TooManyControls) {"InvalidOperationException"} else {"ArgumentException"}}));
        } else {
            output.push(result);
        }
    }
    output
}

fn number(value: &Value, key: &str, default: i32) -> i32 {
    value
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(default)
}
fn boolean(value: &Value, key: &str, default: bool) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(default)
}
fn provider(value: &Value, key: &str) -> Option<pr_platform_win::dialogs::settings::page_view::TextFn> {
    value.get(key).filter(|v| !v.is_null()).map(value_text)
}
fn value_text(value: &Value) -> pr_platform_win::dialogs::settings::page_view::TextFn {
    let text = value.as_str().unwrap_or_default().to_owned();
    std::rc::Rc::new(move || text.clone())
}

/// Serializes registrations and the item list for golden comparison.
pub fn dump(builder: &SettingsPageBuilder) -> Value {
    let items: Vec<_> = builder.items().iter().map(|item| json!({"class":format!("{:?}", item.class),"id":item.id,"text":item.text,"style":item.style,"ex_style":item.ex_style})).collect();
    let texts: Vec<_> = builder
        .texts()
        .iter()
        .map(|(id, provider)| json!([id, provider()]))
        .collect();
    json!({"id_base":builder.id_base(),"items":items,"texts":texts,"notes":builder.notes(),"key_boxes":builder.key_boxes(),"warnings":builder.warning_texts(),"counts":{"items":builder.items().len(),"texts":builder.texts().len(),"elements":builder.elements().len()}})
}
