pub mod cancel;
pub mod fetch;
pub mod info;
pub mod pull;
pub mod status;
pub mod submit;

use crate::job::JobStatus;
use comfy_table::{Attribute, Cell, Color};

#[must_use]
pub(crate) fn status_cell(status: JobStatus, is_tty: bool) -> Cell {
    let (text, color, bold) = match status {
        JobStatus::Queued => ("Queued", Color::Yellow, false),
        JobStatus::Running => ("Running", Color::Cyan, true),
        JobStatus::Finished => ("Finished", Color::Green, false),
        JobStatus::Error => ("Error", Color::Red, true),
        JobStatus::Undefined => ("Undefined", Color::DarkGrey, false),
    };
    let cell = Cell::new(text);
    if !is_tty {
        return cell;
    }
    let cell = cell.fg(color);
    if bold {
        cell.add_attribute(Attribute::Bold)
    } else {
        cell
    }
}
