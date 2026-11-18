//! 常量：默认关键词 / 通知词正则 / 噪声标题 / 默认数据源（对照 `core.mjs` 导出）。

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

/// `core.mjs::DEFAULT_KEYWORDS`（顺序即 pickTags 的命中顺序）。
pub const DEFAULT_KEYWORDS: &[&str] = &[
    "四级", "六级", "四六级", "选课", "补选", "退课", "重选", "考试", "补考", "重修", "缓考",
    "报名", "截止", "讲座", "报告", "成绩", "绩点", "学分", "奖学金", "助学金", "助学贷款",
    "竞赛", "大赛", "实习", "招聘", "宣讲", "保研", "推免", "论文", "答辩", "毕业", "调课",
    "停课", "放假", "假期", "校历", "缴费", "证书", "体测", "体检", "评教", "志愿", "交换",
    "国际交流", "学籍", "转专业", "辅修", "双学位", "宿舍", "校园卡", "图书馆", "实验室",
];

/// `core.mjs::NOTICE_WORDS`。
pub static NOTICE_WORDS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?i)通知|公告|公示|通告|讲座|报告|报名|考试|评选|申报|征集|安排|意见|招标|中标|招聘|宣讲|学术").expect("NOTICE_WORDS"));

/// `core.mjs::NOISE_TITLES`。
pub fn is_noise_title(title: &str) -> bool {
    static NOISE: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
        HashSet::from([
            "更多",
            "查看更多",
            "首页",
            "上一页",
            "下一页",
            "尾页",
            "返回",
            "详情",
            "登录",
            "注册",
            "网站首页",
            "学校主页",
            "联系我们",
            "English",
            "EN",
            "中文",
        ])
    });
    NOISE.contains(title)
}

/// 默认数据源种子（`core.mjs::DEFAULT_SOURCES`），字段与 Node 侧一一对应。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultSource {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub url: &'static str,
    pub page_url: &'static str,
    pub pages: u32,
    pub items_path: &'static str,
    pub map_title: &'static str,
    pub map_id: &'static str,
    pub map_date: &'static str,
    pub map_summary: &'static str,
    pub detail_url_template: &'static str,
    pub content_path: &'static str,
    pub note: &'static str,
}

/// `core.mjs::DEFAULT_SOURCES` 的 4 个一站式接口源。
pub const DEFAULT_SOURCES: &[DefaultSource] = &[
    DefaultSource {
        id: "bjut-portal-1",
        name: "一站式 · 校发通知",
        kind: "json",
        url: "https://my.bjut.edu.cn/site/get_news_list?category_id=1&p=1&size=32",
        page_url: "https://my.bjut.edu.cn/site/get_news_list?category_id=1&p={n}&size=32",
        pages: 2,
        items_path: "d.data",
        map_title: "title",
        map_id: "id",
        map_date: "createdate",
        map_summary: "summary",
        detail_url_template: "https://my.bjut.edu.cn/site/get_news_detail?id={id}",
        content_path: "d.content",
        note: "学校层面的发文（category_id=1）。首次使用需要登录一次：设置页填账号密码，或在数据源里贴 Cookie。",
    },
    DefaultSource {
        id: "bjut-portal-3",
        name: "一站式 · 公示公告",
        kind: "json",
        url: "https://my.bjut.edu.cn/site/get_news_list?category_id=3&p=1&size=32",
        page_url: "https://my.bjut.edu.cn/site/get_news_list?category_id=3&p={n}&size=32",
        pages: 2,
        items_path: "d.data",
        map_title: "title",
        map_id: "id",
        map_date: "createdate",
        map_summary: "summary",
        detail_url_template: "https://my.bjut.edu.cn/site/get_news_detail?id={id}",
        content_path: "d.content",
        note: "评奖评优、名单公示等（category_id=3）。",
    },
    DefaultSource {
        id: "bjut-portal-4",
        name: "一站式 · 教学通告",
        kind: "json",
        url: "https://my.bjut.edu.cn/site/get_news_list?category_id=4&p=1&size=32",
        page_url: "https://my.bjut.edu.cn/site/get_news_list?category_id=4&p={n}&size=32",
        pages: 2,
        items_path: "d.data",
        map_title: "title",
        map_id: "id",
        map_date: "createdate",
        map_summary: "summary",
        detail_url_template: "https://my.bjut.edu.cn/site/get_news_detail?id={id}",
        content_path: "d.content",
        note: "教务处 / 研究生院 / 都柏林学院的教学类通知（category_id=4）。",
    },
    DefaultSource {
        id: "bjut-portal-6",
        name: "一站式 · 院部处通知",
        kind: "json",
        url: "https://my.bjut.edu.cn/site/get_news_list?category_id=6&p=1&size=32",
        page_url: "https://my.bjut.edu.cn/site/get_news_list?category_id=6&p={n}&size=32",
        pages: 3,
        items_path: "d.data",
        map_title: "title",
        map_id: "id",
        map_date: "createdate",
        map_summary: "summary",
        detail_url_template: "https://my.bjut.edu.cn/site/get_news_detail?id={id}",
        content_path: "d.content",
        note: "各院部处的通知，量最大（category_id=6）。",
    },
];
