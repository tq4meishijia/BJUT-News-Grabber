//! 崩溃安全：1000 次随机偏移截断写入模拟，store.json 始终可解析（§9 P3 出门门槛）。

use std::path::PathBuf;

use cn_store::{load_store, save_store, save_store_crashable, StoreData};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cn-crash-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

/// 固定种子 xorshift。
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
}

#[test]
fn crash_1000_random_truncations_keep_store_parseable() {
    let dir = temp_dir("1000");
    let path = dir.join("store.json");

    // v0：初始合法状态
    let mut v0 = StoreData::default();
    v0.settings.poll_minutes = 30;
    save_store(&path, &v0).expect("save v0");

    // 准备一份较大的 v1 快照（多条 notice，保证截断有长度可切）
    let mut v1 = v0.clone();
    for i in 0..50 {
        v1.notices.push(cn_store::NoticeRecord {
            id: format!("n-{i}"),
            title: format!("通知 {i}：关于考试安排的通知"),
            content: "正文".repeat(50),
            ..Default::default()
        });
    }
    let snapshot_len = serde_json::to_string(&v1).unwrap().len();
    assert!(snapshot_len > 10_000, "快照应足够大：{snapshot_len}");

    let mut rng = Rng(0xDEADBEEFCAFEBABE);
    for round in 0..1000u32 {
        let cut = (rng.next() % snapshot_len as u64) as usize;
        // 模拟写到一半被杀：截断写 tmp，不 rename
        save_store_crashable(&path, &v1, Some(cut)).expect("crash write");
        // 目标文件必须仍是 v0（可解析）
        let loaded = load_store(&path);
        assert_eq!(
            loaded.settings.poll_minutes, 30,
            "第 {round} 轮截断后 store.json 被破坏（cut={cut}）"
        );
        assert_eq!(loaded.notices.len(), 0, "第 {round} 轮截断后旧数据丢失");
    }

    // 最后一次完整写入成功 → v1 落盘
    save_store(&path, &v1).expect("save v1");
    let loaded = load_store(&path);
    assert_eq!(loaded.notices.len(), 50);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corrupted_file_falls_back_to_default() {
    let dir = temp_dir("corrupt");
    let path = dir.join("store.json");
    std::fs::write(&path, "{not json!!").expect("write garbage");
    let loaded = load_store(&path);
    assert_eq!(loaded.version, 1);
    assert!(!loaded.sources.is_empty(), "损坏文件回退默认种子");
    let _ = std::fs::remove_dir_all(&dir);
}
