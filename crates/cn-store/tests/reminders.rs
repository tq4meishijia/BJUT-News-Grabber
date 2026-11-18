//! 提醒引擎四边界（due-1min / due / due+5min / due+6min）+ fired 去重（B6）。

use std::sync::Arc;

use chrono::{Local, TimeZone};
use cn_model::FixedClock;
use cn_store::reminders::check_reminders;
use cn_store::{ItemRecord, ReminderRecord};

fn clock_at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Arc<FixedClock> {
    Arc::new(FixedClock::new(
        Local
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .expect("valid dt"),
    ))
}

fn item_due(due_at: &str) -> ItemRecord {
    ItemRecord {
        id: "s-1".into(),
        title: "交报告".into(),
        due_at: due_at.into(),
        remind_before: vec![1], // level = 1 分钟 → 窗口 [due-1min, due+5min]
        ..Default::default()
    }
}

fn run(now: &FixedClock, items: &mut [ItemRecord]) -> (bool, Vec<ReminderRecord>, Vec<(String, String)>) {
    let mut reminders: Vec<ReminderRecord> = Vec::new();
    let mut fired: Vec<(String, String)> = Vec::new();
    let mut counter = 0u64;
    let changed = check_reminders(
        items,
        &mut reminders,
        true,
        now,
        &mut counter,
        &mut |t: &str, l: &str| {
            fired.push((t.to_string(), l.to_string()));
            true
        },
        &mut |_log: &str| {},
    );
    (changed, reminders, fired)
}

#[test]
fn boundary_due_minus_1min_fires() {
    // 到期 12:00，now = 11:59 = due-1min → 触发（at = due - level）
    let now = clock_at(2026, 11, 18, 11, 59);
    let mut items = [item_due("2026-11-18 12:00")];
    let (changed, reminders, fired) = run(&now, &mut items);
    assert!(changed, "due-1min 应触发");
    assert_eq!(fired.len(), 1);
    assert_eq!(reminders.len(), 1);
    assert!(items[0].fired.contains(&1));
}

#[test]
fn boundary_due_fires() {
    // now = 12:00 → 触发
    let now = clock_at(2026, 11, 18, 12, 0);
    let mut items = [item_due("2026-11-18 12:00")];
    let (changed, _reminders, fired) = run(&now, &mut items);
    assert!(changed, "due 时刻应触发");
    assert_eq!(fired.len(), 1);
}

#[test]
fn boundary_due_plus_5min_fires() {
    // now = 12:05 = due+5min → 仍触发（窗口含端点）
    let now = clock_at(2026, 11, 18, 12, 5);
    let mut items = [item_due("2026-11-18 12:00")];
    let (changed, _reminders, fired) = run(&now, &mut items);
    assert!(changed, "due+5min 应触发");
    assert_eq!(fired.len(), 1);
}

#[test]
fn boundary_due_plus_6min_skips() {
    // now = 12:06 = due+6min → 过窗不触发
    let now = clock_at(2026, 11, 18, 12, 6);
    let mut items = [item_due("2026-11-18 12:00")];
    let (changed, _reminders, fired) = run(&now, &mut items);
    assert!(!changed, "due+6min 不应触发");
    assert!(fired.is_empty());
    assert!(items[0].fired.is_empty());
}

#[test]
fn fired_dedup_no_double_fire() {
    // 第一次触发后 fired 含 level，再次检查不重复
    let now = clock_at(2026, 11, 18, 12, 0);
    let mut items = [item_due("2026-11-18 12:00")];
    let (changed1, _, fired1) = run(&now, &mut items);
    assert!(changed1 && fired1.len() == 1);
    let (changed2, _, fired2) = run(&now, &mut items);
    assert!(!changed2, "同一 level 不应二次触发");
    assert!(fired2.is_empty());
}

#[test]
fn multi_level_fires_each_once() {
    let now = clock_at(2026, 11, 18, 12, 0);
    let mut item = item_due("2026-11-18 12:00");
    item.remind_before = vec![1, 0]; // 提前 1 分钟 + 准时
    let mut items = [item];
    let (changed, reminders, fired) = run(&now, &mut items);
    assert!(changed);
    assert_eq!(fired.len(), 2, "两个 level 各触发一次");
    assert_eq!(reminders.len(), 2);
    // 再跑不再触发
    let (changed2, _, _) = run(&now, &mut items);
    assert!(!changed2);
}

#[test]
fn done_item_and_no_due_skipped() {
    let now = clock_at(2026, 11, 18, 12, 0);
    let mut done_item = item_due("2026-11-18 12:00");
    done_item.done = true;
    let no_due = ItemRecord {
        id: "s-2".into(),
        ..item_due("")
    };
    let mut items = [done_item, no_due];
    let (changed, _, fired) = run(&now, &mut items);
    assert!(!changed);
    assert!(fired.is_empty());
}

#[test]
fn reminder_storage_cap_40() {
    let now = clock_at(2026, 11, 18, 12, 0);
    let mut items: Vec<ItemRecord> = (0..50)
        .map(|i| {
            let mut it = item_due("2026-11-18 12:00");
            it.id = format!("s-{i}");
            it.title = format!("任务{i}");
            it.remind_before = vec![1];
            it
        })
        .collect();
    let mut reminders: Vec<ReminderRecord> = Vec::new();
    let mut counter = 0u64;
    check_reminders(
        &mut items,
        &mut reminders,
        true,
        now.as_ref(),
        &mut counter,
        &mut |_, _| true,
        &mut |_| {},
    );
    assert_eq!(reminders.len(), 40, "meta.reminders 存储上限 40");
}

#[test]
fn label_contains_countdown_and_stamp() {
    let now = clock_at(2026, 11, 18, 11, 59);
    let mut items = [item_due("2026-11-18 12:00")];
    let (_c, _r, fired) = run(&now, &mut items);
    let label = &fired[0].1;
    assert!(label.contains("提前1 分钟前"), "label={label}");
    assert!(label.contains("11月18日 12:00"), "label={label}");
    assert!(label.contains("到期"), "label={label}");
}
