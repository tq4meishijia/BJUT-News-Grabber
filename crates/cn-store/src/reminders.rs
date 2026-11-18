//! 提醒引擎（§4.2 / B6）：窗口 [due-level, due+5min]，fired[] 去重，Clock 注入。

use chrono::{DateTime, Datelike, Local, Timelike};

use cn_model::Clock;

use crate::state::{ItemRecord, ReminderRecord};

/// `store.mjs::checkReminders`：返回是否有新触发。
/// 判定：`now >= due - level` 且 `now <= due + 5min` 且 `level ∉ fired[]`。
pub fn check_reminders(
    items: &mut [ItemRecord],
    reminders: &mut Vec<ReminderRecord>,
    notify_enabled: bool,
    clock: &dyn Clock,
    counter: &mut u64,
    on_notify: &mut dyn FnMut(&str, &str) -> bool,
    on_log: &mut dyn FnMut(&str),
) -> bool {
    let now = clock.now();
    let now_ms = now.timestamp_millis();
    let mut changed = false;
    for item in items.iter_mut() {
        if item.done || item.due_at.is_empty() {
            continue;
        }
        let due_iso = cn_core::normalize_due(&item.due_at).unwrap_or_default();
        if due_iso.is_empty() {
            continue;
        }
        let Some(due) = cn_core::parse_due_local(&due_iso) else {
            continue;
        };
        let due_ms = due.timestamp_millis();
        let levels: Vec<u32> = if item.remind_before.is_empty() {
            vec![30]
        } else {
            item.remind_before.clone()
        };
        for minutes in levels {
            if item.fired.contains(&minutes) {
                continue;
            }
            let at = due_ms - minutes as i64 * 60_000;
            if now_ms < at {
                continue;
            }
            if now_ms > due_ms + 5 * 60_000 {
                continue;
            }
            item.fired.push(minutes);
            let stamp = format!(
                "{}月{}日 {:02}:{:02}",
                due.month(),
                due.day(),
                due.hour(),
                due.minute()
            );
            let prefix = if minutes > 0 {
                format!("提前{}：", cn_core::describe_minutes(minutes as i64))
            } else {
                String::new()
            };
            let suffix = if minutes > 0 {
                format!("（{}）", cn_core::countdown_text(&item.due_at, now))
            } else {
                " 到点了".to_string()
            };
            let label = format!("{prefix}{stamp} 到期{suffix}");
            record_reminder(
                item,
                &label,
                reminders,
                notify_enabled,
                now,
                counter,
                on_notify,
                on_log,
            );
            changed = true;
        }
    }
    changed
}

/// `store.mjs::recordReminder`：写 meta.reminders（前 40）+ 通知回调。
#[allow(clippy::too_many_arguments)]
fn record_reminder(
    item: &ItemRecord,
    label: &str,
    reminders: &mut Vec<ReminderRecord>,
    notify_enabled: bool,
    now: DateTime<Local>,
    counter: &mut u64,
    on_notify: &mut dyn FnMut(&str, &str) -> bool,
    on_log: &mut dyn FnMut(&str),
) {
    *counter += 1;
    let entry = ReminderRecord {
        id: crate::state::new_id("r", &item.id, now.timestamp_millis(), *counter),
        item_id: item.id.clone(),
        title: item.title.clone(),
        note: label.to_string(),
        due_at: item.due_at.clone(),
        at: now.to_rfc3339(),
    };
    reminders.insert(0, entry);
    reminders.truncate(40);
    if notify_enabled {
        on_notify(&format!("日程提醒 · {}", item.title), label);
    }
    on_log(&format!("[提醒] {} —— {}", item.title, label));
}
