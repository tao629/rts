//! 图形验证码生成。
//!
//! 自绘 SVG，不依赖任何图形处理 crate（spec §2 与 intent 的约束）：
//! 生成 4 位字符答案 → 渲染成 260×60 的 SVG → base64 编码为 data URI 随 JSON 返回。
//!
//! 安全约定：
//! - **答案只存在于服务端内存**，绝不进入响应体，也绝不写进日志；
//! - 字符集剔除 `0/O/1/I/L` 等易混字符，降低人工识别时的歧义。

pub mod store;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::Rng;

/// 验证码字符集：剔除 `0`、`O`、`1`、`I`、`L` 等易混字符。
pub const CAPTCHA_CHARSET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
/// 验证码长度（字符数）。
pub const CAPTCHA_LENGTH: usize = 4;
/// 验证码图片宽度（像素）。
///
/// 取 260 是为了与登录页里的验证码输入框等宽：图片按 `object-contain` 缩放后
/// 恰好铺满输入框宽度，字符不会被拉伸变形。
pub const CAPTCHA_WIDTH: u32 = 260;
/// 验证码图片高度（像素）。
pub const CAPTCHA_HEIGHT: u32 = 60;

/// data URI 前缀。
pub const DATA_URI_PREFIX: &str = "data:image/svg+xml;base64,";

/// 字符配色：深色系，保证与浅色背景有足够对比度。
const PALETTE: [&str; 6] = [
    "#1f3a68", "#8c2f39", "#1f6f4a", "#5b3a8c", "#a05a12", "#245a6b",
];

/// 一次验证码生成的结果。
///
/// `answer` 只在服务端内部流转（写入验证码存储），调用方**不得**把它放进响应体。
#[derive(Debug, Clone)]
pub struct GeneratedCaptcha {
    /// 验证码答案（大写字母 / 数字）。
    pub answer: String,
    /// 可直接用于 `<img src>` 的 data URI。
    pub image: String,
}

/// 生成一份验证码：随机答案 + SVG 图片。
pub fn generate() -> GeneratedCaptcha {
    let answer = random_answer();
    let image = to_data_uri(&render_svg(&answer));
    GeneratedCaptcha { answer, image }
}

/// 生成随机答案。
pub fn random_answer() -> String {
    let mut rng = rand::rng();
    (0..CAPTCHA_LENGTH)
        .map(|_| {
            let index = rng.random_range(0..CAPTCHA_CHARSET.len());
            CAPTCHA_CHARSET[index] as char
        })
        .collect()
}

/// 把 SVG 源码包装成 base64 data URI。
pub fn to_data_uri(svg: &str) -> String {
    format!("{DATA_URI_PREFIX}{}", STANDARD.encode(svg.as_bytes()))
}

/// 把 SVG 源码渲染成图片。
///
/// 加随机干扰线与噪点、字符随机旋转与错位，目的是让"脚本直接读取 SVG 文本"
/// 变得不轻松（OCR 本次不在对抗范围内，见 plan.md 风险表）。
pub fn render_svg(answer: &str) -> String {
    let mut rng = rand::rng();
    let mut svg = String::with_capacity(2048);

    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{CAPTCHA_WIDTH}\" height=\"{CAPTCHA_HEIGHT}\" viewBox=\"0 0 {CAPTCHA_WIDTH} {CAPTCHA_HEIGHT}\">"
    ));
    svg.push_str(&format!(
        "<rect width=\"{CAPTCHA_WIDTH}\" height=\"{CAPTCHA_HEIGHT}\" fill=\"#f2f5fa\"/>"
    ));

    // 干扰线：横跨画布的随机线段。
    for _ in 0..5 {
        let x1 = rng.random_range(0..CAPTCHA_WIDTH);
        let y1 = rng.random_range(0..CAPTCHA_HEIGHT);
        let x2 = rng.random_range(0..CAPTCHA_WIDTH);
        let y2 = rng.random_range(0..CAPTCHA_HEIGHT);
        let color = PALETTE[rng.random_range(0..PALETTE.len())];
        svg.push_str(&format!(
            "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" stroke=\"{color}\" stroke-width=\"1\" opacity=\"0.35\"/>"
        ));
    }

    // 噪点：细碎的小圆点。
    for _ in 0..60 {
        let cx = rng.random_range(0..CAPTCHA_WIDTH);
        let cy = rng.random_range(0..CAPTCHA_HEIGHT);
        let radius = rng.random_range(0.6..1.6);
        let color = PALETTE[rng.random_range(0..PALETTE.len())];
        svg.push_str(&format!(
            "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{radius:.1}\" fill=\"{color}\" opacity=\"0.5\"/>"
        ));
    }

    // 字符：等分横向空间，逐个随机旋转、上下错位。
    let step = CAPTCHA_WIDTH as f64 / (CAPTCHA_LENGTH as f64 + 1.0);
    for (index, ch) in answer.chars().enumerate() {
        let x = step * (index as f64 + 1.0);
        let y = CAPTCHA_HEIGHT as f64 / 2.0 + rng.random_range(-4.0..8.0);
        let rotate = rng.random_range(-25.0..25.0);
        let color = PALETTE[rng.random_range(0..PALETTE.len())];
        svg.push_str(&format!(
            "<text x=\"{x:.1}\" y=\"{y:.1}\" font-family=\"DejaVu Sans Mono, Consolas, monospace\" font-size=\"34\" font-weight=\"bold\" fill=\"{color}\" text-anchor=\"middle\" transform=\"rotate({rotate:.1} {x:.1} {y:.1})\">{ch}</text>"
        ));
    }

    svg.push_str("</svg>");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_only_uses_charset_and_has_expected_length() {
        for _ in 0..64 {
            let answer = random_answer();
            assert_eq!(answer.chars().count(), CAPTCHA_LENGTH);
            for ch in answer.chars() {
                assert!(
                    CAPTCHA_CHARSET.contains(&(ch as u8)),
                    "字符 {ch} 不在字符集内"
                );
            }
        }
    }

    #[test]
    fn charset_excludes_confusable_characters() {
        for excluded in ['0', 'O', '1', 'I', 'L'] {
            assert!(
                !CAPTCHA_CHARSET.contains(&(excluded as u8)),
                "字符集不应包含 {excluded}"
            );
        }
    }

    #[test]
    fn image_is_base64_data_uri() {
        let generated = generate();
        assert!(generated.image.starts_with(DATA_URI_PREFIX));
        let encoded = generated
            .image
            .strip_prefix(DATA_URI_PREFIX)
            .expect("必须带 data URI 前缀");
        let decoded = STANDARD.decode(encoded).expect("base64 应当可以解码");
        let svg = String::from_utf8(decoded).expect("SVG 必须是 UTF-8");
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn svg_contains_all_answer_characters() {
        let answer = random_answer();
        let svg = render_svg(&answer);
        for ch in answer.chars() {
            assert!(svg.contains(&format!(">{ch}<")), "SVG 中缺少字符 {ch}");
        }
    }
}
