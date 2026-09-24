use crate::api::ApiSpace;
use crate::ui;

pub(super) fn print_space_row(id: &str, name: &str, mapped: bool, detail: &str) {
    let marker = if mapped { "*" } else { " " };
    println!("{} {:<4} {:<24} {}", marker, id, name, ui::dim(detail));
}

pub(super) fn space_json(space: &ApiSpace, sync_dir: Option<&String>) -> serde_json::Value {
    serde_json::json!({
        "id": space.id,
        "name": space.name,
        "description": space.description,
        "role": space.role,
        "sync_dir": sync_dir,
    })
}
