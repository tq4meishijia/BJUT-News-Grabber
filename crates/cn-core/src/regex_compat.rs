//! fancy-regex 唯一入口：需要向前/向后断言的正则只能出现在本模块。
//!
//! 依赖方向约束：`cn-core` 其余模块只依赖本模块的 [`LookRegex`] / [`LookCaps`]，
//! 将来若替换断言实现（例如改写成"匹配后手动检查下一字符"）只改这一个文件。

use fancy_regex::Regex as Fancy;

/// 带断言能力的正则（JS `RegExp.exec` 语义：最左匹配）。
pub struct LookRegex {
    inner: Fancy,
}

/// 一次捕获结果。`groups[0]` 为整体匹配，`groups[i]` 为第 i 个捕获组；
/// 未参与匹配的组为 `None`。`js_len` 对应 JS `match.length`（组数 + 1）。
#[derive(Debug, Clone)]
pub struct LookCaps {
    /// 整体匹配的字节区间。
    pub start: usize,
    pub end: usize,
    pub groups: Vec<Option<String>>,
}

impl LookCaps {
    /// 第 i 组文本（0 = 整体匹配）。
    pub fn get(&self, i: usize) -> Option<&str> {
        self.groups.get(i).and_then(|g| g.as_deref())
    }

    /// JS `hit.length`：捕获组数 + 1。
    pub fn js_len(&self) -> usize {
        self.groups.len()
    }
}

impl LookRegex {
    /// 编译正则；模式错误视为编程错误直接 panic。
    pub fn new(pattern: &str) -> Self {
        let inner = Fancy::new(pattern)
            .unwrap_or_else(|err| panic!("invalid look-around pattern `{pattern}`: {err}"));
        Self { inner }
    }

    /// 最左匹配（对应 JS `RegExp.exec` 无 `/g`）。
    pub fn captures(&self, text: &str) -> Option<LookCaps> {
        match self.inner.captures(text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).expect("group 0 always participates");
                let mut groups = vec![Some(whole.as_str().to_string())];
                for i in 1..caps.len() {
                    groups.push(caps.get(i).map(|m| m.as_str().to_string()));
                }
                Some(LookCaps {
                    start: whole.start(),
                    end: whole.end(),
                    groups,
                })
            }
            _ => None,
        }
    }

    /// 是否命中（不取捕获）。
    pub fn is_match(&self, text: &str) -> bool {
        matches!(self.inner.is_match(text), Ok(true))
    }
}
