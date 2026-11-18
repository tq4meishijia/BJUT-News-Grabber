//! 编码嗅探与解码：GBK / UTF-8 / BOM 三种解码，GBK 5 样例零乱码。
//! GBK 样例用 encoding_rs::GBK 编码（GBK 兼容输出），decode_body 往返一致。

use cn_net::{decode_body, sniff_charset};

fn gbk(text: &str) -> Vec<u8> {
    let (cow, _, _) = encoding_rs::GBK.encode(text);
    cow.into_owned()
}

#[test]
fn sniff_charset_priority() {
    // header 优先
    assert_eq!(
        sniff_charset("text/html; charset=GBK", b"<meta charset=\"utf-8\">"),
        "gbk"
    );
    // BOM 其次
    assert_eq!(sniff_charset("", &[0xef, 0xbb, 0xbf, b'<']), "utf-8");
    assert_eq!(sniff_charset("", &[0xff, 0xfe, b'<', 0]), "utf-16le");
    assert_eq!(sniff_charset("", &[0xfe, 0xff, 0, b'<']), "utf-16be");
    // meta 再次
    assert_eq!(
        sniff_charset("", b"head><meta http-equiv=Content-Type content=\"text/html; charset=gb2312\">".as_ref()),
        "gb2312"
    );
    // 默认 utf-8
    assert_eq!(sniff_charset("text/plain", b"hello"), "utf-8");
}

#[test]
fn gbk_5_samples_zero_mojibake() {
    let samples = [
        "关于2026年国庆节放假安排的通知",
        "关于开展2026级本科生学籍注册工作的通知",
        "关于做好2026年度国家助学金申请工作的通知",
        "关于组织学生体质健康测试的通知",
        "关于校园卡系统升级维护的通知（联系电话：010-67393333）",
    ];
    for (i, text) in samples.iter().enumerate() {
        let bytes = gbk(text);
        // 同一批字节，三种 charset 线索都要解出原文
        for ct in ["text/html; charset=gbk", "text/html; charset=gb2312", ""] {
            let out = if ct.is_empty() {
                // 无 header 时走 meta：包一层 meta 声明
                let wrapped = format!(
                    "<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=gb2312\"></head><body>{}</body></html>",
                    text
                );
                decode_body(&gbk(&wrapped), "")
            } else {
                decode_body(&bytes, ct)
            };
            assert!(out.contains(text), "样本#{i} ct={ct:?} 解码乱码：{out}");
        }
    }
}

#[test]
fn utf8_and_utf16_decode() {
    // UTF-8
    let s = "关于考试安排的通知";
    let (cow, _, _) = encoding_rs::UTF_8.encode(s);
    assert_eq!(decode_body(&cow, "text/html; charset=utf-8"), s);
    // UTF-16LE/BE 带 BOM（WHATWG Encoding::encode 对 UTF-16 的输出编码是 UTF-8，
    // 所以 UTF-16 字节手工按码元构造）。
    for big_endian in [false, true] {
        let mut with_bom: Vec<u8> = if big_endian {
            vec![0xfe, 0xff]
        } else {
            vec![0xff, 0xfe]
        };
        for unit in s.encode_utf16() {
            let bytes = unit.to_be_bytes();
            if big_endian {
                with_bom.extend_from_slice(&bytes);
            } else {
                with_bom.extend_from_slice(&unit.to_le_bytes());
            }
        }
        assert_eq!(decode_body(&with_bom, ""), s, "be={big_endian}");
    }
    // 非法字节 lossy 不 panic
    let _ = decode_body(&[0xff, 0xfe, 0x00], "text/html; charset=utf-8");
}

#[test]
fn meta_sniff_on_latin1_head() {
    // meta charset 在前 3000 字节内才能被看到（对照 JS subarray(0,3000)）
    let mut body = "x".repeat(3100).into_bytes();
    body.extend_from_slice(b"<meta charset=\"gbk\">");
    assert_eq!(sniff_charset("", &body), "utf-8"); // 超出窗口，看不到
    let mut body2 = "x".repeat(100).into_bytes();
    body2.extend_from_slice(b"<meta charset=\"gbk\">");
    assert_eq!(sniff_charset("", &body2), "gbk");
}
