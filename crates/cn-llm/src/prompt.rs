//! 提示词构造（§6.1 三任务合并输出）与 PROMPT_VERSION 冻结。

/// 提示词版本；换提示词自动让缓存失效。P0 冻结为 1。
pub const PROMPT_VERSION: u32 = 1;

/// system prompt（冻结文本，与 llm.mjs 一致 + 三任务扩展）。
pub fn system_prompt() -> &'static str {
    "你是一位在校大学生的私人助理。任务：把校内通知里「与我有关、需要我动作」的部分挑出来，用中文写清楚。\
严格只输出 JSON 数组，不要解释、不要 markdown 代码块。"
}

/// 单条通知的输入形状（title / date / content[0..900]）。
pub struct PromptNotice<'a> {
    pub title: &'a str,
    pub published_at: &'a str,
    pub content: &'a str,
}

/// `llm.mjs::buildPrompt` + §6.1 三任务字段扩展。
pub fn build_prompt(notices: &[PromptNotice<'_>]) -> String {
    let payload: Vec<serde_json::Value> = notices
        .iter()
        .enumerate()
        .map(|(index, n)| {
            let content: String = n.content.chars().take(900).collect();
            serde_json::json!({
                "i": index,
                "title": n.title,
                "date": n.published_at,
                "content": content,
            })
        })
        .collect();
    format!(
        "下面是一批校园通知。对每一条输出一个对象：\n\
{{\"i\":序号,\"category\":\"教务|考试|评奖资助|讲座学术|就业实习|后勤生活|招标采购|其他\",\"gist\":\"一句话讲清这件事（不超过60字）\",\
\"useful\":[\"对我有用的点，每条不超过30字，最多3条\"],\"tags\":[\"考试\",\"报名\",\"讲座\"等2-4个标签],\
\"deadline\":\"YYYY-MM-DDTHH:mm 或 空字符串\",\"contact\":\"联系人与电话，没有则空\",\"audience\":\"本科生|研究生|全体|教师|不限\",\
\"relevant\":true|false,\"confidence\":0.0-1.0}}\n\
判断标准：只和老师/行政/后勤有关、或者纯招标采购类，relevant 给 false 且 useful 给空数组。\
有明确截止时间的，deadline 必须填；没有就给空字符串。\n\n通知列表：\n{}",
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    )
}
