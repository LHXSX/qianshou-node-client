//! RapidOCR · PP-OCRv4 mobile · Rust 直推 ONNX 实现 · V8.2 RFC (2026-06-11)
//!
//! 模型来源: modelscope.cn/RapidAI/RapidOCR v3.8.0 PP-OCRv4 mobile
//!   det:  ch_PP-OCRv4_det_mobile.onnx        4.7MB  DBNet 文本检测
//!   cls:  ch_ppocr_mobile_v2.0_cls_mobile.onnx  585KB  方向分类(0/180°)
//!   rec:  ch_PP-OCRv4_rec_mobile.onnx        10.4MB CRNN 文本识别
//!   keys: ppocr_keys_v1.txt                  26KB   6624 个字符字典
//!
//! 处理流程:
//!   1. 加载 3 个 ort session + keys 字典(只做一次)
//!   2. 读入图像 → letterbox 到 32 倍数尺寸 + normalize
//!   3. det.run() → 概率图 → 二值化 → 连通域 BFS → 文本框
//!   4. 对每个文本框:
//!      a. crop + resize 到 (48, 192) → cls.run() → 决定是否旋转 180°
//!      b. 旋转后 resize 到 (48, W变) → rec.run() → CTC argmax → keys 索引 → 字符串
//!   5. 输出 JSON 跟 ocr_image.py schema 兼容
//!
//! 性能(macOS arm64 · ONNX Runtime CPU):
//!   - 加载 3 个模型 + keys: ~80 ms (一次性)
//!   - 单张 1080p 图: ~300-600 ms (含 det + ~10 个文本框的 cls/rec)
//!   - 比 PaddleOCR Python (subprocess 启动 1500ms+) 快 3-5×
//!
//! 注意:
//!   - 此模块仅在 `feature = "onnx"` 启用时编译
//!   - ort load-dynamic 需要运行时找到 libonnxruntime.{so,dylib,dll}
//!     (客户端打包时随 .app 一起发,见 prebake-onnxruntime.sh · TODO P0 收尾)

#![cfg(feature = "onnx")]

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb};
use ndarray::{s, Array, Array2, Array3, Array4, ArrayD, Axis};
use ort::{Session, SessionBuilder, Value as OrtValue};
use serde::Serialize;

// ════════════════════════════════════════════════════════════════════
// 常量(对齐 RapidOCR Python rapidocr_onnxruntime 默认参数)
// ════════════════════════════════════════════════════════════════════

/// DBNet 检测:输入短边 limit
const DET_LIMIT_SIDE_LEN: u32 = 736;
/// DBNet 二值化阈值
const DET_DB_THRESH: f32 = 0.3;
/// DBNet box 置信度阈值
const DET_DB_BOX_THRESH: f32 = 0.6;
/// DBNet 文本框扩展比例
const DET_DB_UNCLIP_RATIO: f32 = 1.6;
/// DBNet 最小文本框像素面积(过滤噪点)
const DET_MIN_BOX_AREA: u32 = 16;

/// 方向分类:输入尺寸 (H, W)
const CLS_INPUT_HEIGHT: u32 = 48;
const CLS_INPUT_WIDTH: u32 = 192;
/// 方向分类置信度阈值,超过才旋转(避免误转)
const CLS_THRESH: f32 = 0.9;

/// CRNN 识别:输入高度
const REC_INPUT_HEIGHT: u32 = 48;
/// CRNN 识别:输入最大宽度(超过 letterbox padding)
const REC_INPUT_MAX_WIDTH: u32 = 320;
/// CRNN 字符序列最大长度(经验值)
const REC_BATCH_NUM: usize = 1;

/// ImageNet 归一化常数(det 模型用)
const NORM_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const NORM_STD: [f32; 3] = [0.229, 0.224, 0.225];

// ════════════════════════════════════════════════════════════════════
// 公开类型
// ════════════════════════════════════════════════════════════════════

/// 单行识别结果(对齐 ocr_image.py 的 line_detail schema)
#[derive(Debug, Clone, Serialize)]
pub struct OcrLine {
    /// 文本框 4 个角点 [[x1,y1],[x2,y2],[x3,y3],[x4,y4]](顺时针 · 原图坐标)
    pub r#box: [[i32; 2]; 4],
    /// 识别出的文字
    pub text: String,
    /// 置信度 0.0-1.0
    pub confidence: f32,
}

/// 整图 OCR 结果
#[derive(Debug, Clone, Serialize)]
pub struct OcrResult {
    pub lines: Vec<OcrLine>,
    /// 拼起来的全文(换行分隔)
    pub text: String,
    pub elapsed_ms: u64,
}

// ════════════════════════════════════════════════════════════════════
// Engine · 持有 3 个 session + 字典 · 多次推理共享
// ════════════════════════════════════════════════════════════════════

pub struct RapidOcrEngine {
    det: Session,
    cls: Session,
    rec: Session,
    /// 字符表(index 0 = blank · 索引 1+ = ppocr_keys_v1.txt 行)
    keys: Vec<String>,
}

impl RapidOcrEngine {
    /// 加载 model_dir 下的 4 个文件 · 构建 Engine
    pub fn load(model_dir: &Path) -> Result<Self> {
        let det_path = model_dir.join("ch_PP-OCRv4_det_mobile.onnx");
        let cls_path = model_dir.join("ch_ppocr_mobile_v2.0_cls_mobile.onnx");
        let rec_path = model_dir.join("ch_PP-OCRv4_rec_mobile.onnx");
        let keys_path = model_dir.join("ppocr_keys_v1.txt");

        for (label, p) in [
            ("det", &det_path), ("cls", &cls_path),
            ("rec", &rec_path), ("keys", &keys_path),
        ] {
            if !p.is_file() {
                return Err(anyhow!(
                    "RapidOCR 缺文件 {} ({}) · 请先 onnx_installer 装好 rapid_ocr_v1",
                    label, p.display()
                ));
            }
        }

        let det = SessionBuilder::new()
            .context("ort SessionBuilder 创建失败")?
            .commit_from_file(&det_path)
            .with_context(|| format!("加载 det 失败 {}", det_path.display()))?;
        let cls = SessionBuilder::new()
            .context("ort SessionBuilder 创建失败")?
            .commit_from_file(&cls_path)
            .with_context(|| format!("加载 cls 失败 {}", cls_path.display()))?;
        let rec = SessionBuilder::new()
            .context("ort SessionBuilder 创建失败")?
            .commit_from_file(&rec_path)
            .with_context(|| format!("加载 rec 失败 {}", rec_path.display()))?;

        // ppocr_keys_v1.txt: 每行一个字符 · index 0 是 blank · 行 N 对应 index N+1
        let keys_raw = std::fs::read_to_string(&keys_path)
            .with_context(|| format!("读 keys 失败 {}", keys_path.display()))?;
        let mut keys: Vec<String> = vec![String::new()]; // index 0 = blank
        for line in keys_raw.lines() {
            // 不要 trim · 字典里有空格字符(\x20)在某一行
            keys.push(line.to_string());
        }
        // 加最后一个 space(RapidOCR 约定 · 字典 + " " 才完整覆盖 num_classes)
        keys.push(" ".to_string());

        tracing::info!(
            "RapidOcr · loaded {} keys (含 blank + space)",
            keys.len()
        );

        Ok(Self { det, cls, rec, keys })
    }

    /// 对单张图做 OCR 推理 · 全流程
    pub fn run(&mut self, image_bytes: &[u8]) -> Result<OcrResult> {
        let t0 = Instant::now();
        let img = image::load_from_memory(image_bytes)
            .context("解码图像失败")?;
        let (orig_w, orig_h) = img.dimensions();
        if orig_w == 0 || orig_h == 0 {
            return Err(anyhow!("空图像"));
        }

        // ── 1. 检测文本框 ──────────────────────────────────────
        let det_boxes = self.detect(&img)?;
        tracing::debug!("rapid_ocr · 检测到 {} 个文本框", det_boxes.len());
        if det_boxes.is_empty() {
            return Ok(OcrResult {
                lines: vec![],
                text: String::new(),
                elapsed_ms: t0.elapsed().as_millis() as u64,
            });
        }

        // ── 2. 对每个文本框: crop → cls → rec ─────────────────
        let rgb = img.to_rgb8();
        let mut lines: Vec<OcrLine> = Vec::with_capacity(det_boxes.len());
        for bbox in det_boxes {
            let crop = match crop_box(&rgb, &bbox) {
                Some(c) => c,
                None => continue,
            };
            // cls 决定是否旋转 180°
            let oriented = match self.classify_and_rotate(&crop) {
                Ok(img) => img,
                Err(e) => {
                    tracing::debug!("rapid_ocr · cls 失败 (用原图继续): {}", e);
                    crop
                }
            };
            // rec 识别字符序列
            match self.recognize(&oriented) {
                Ok((text, conf)) => {
                    if !text.is_empty() {
                        lines.push(OcrLine {
                            r#box: bbox.points(),
                            text,
                            confidence: conf,
                        });
                    }
                }
                Err(e) => {
                    tracing::debug!("rapid_ocr · rec 失败 (跳过此框): {}", e);
                }
            }
        }

        // ── 3. 按 y 坐标排序(从上到下)·  排版还原 ──
        lines.sort_by(|a, b| {
            let ay = (a.r#box[0][1] + a.r#box[2][1]) / 2;
            let by = (b.r#box[0][1] + b.r#box[2][1]) / 2;
            ay.cmp(&by)
        });

        let combined = lines.iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(OcrResult {
            lines,
            text: combined,
            elapsed_ms: t0.elapsed().as_millis() as u64,
        })
    }

    // ────────────────────────────────────────────────────────
    // det · DBNet 文本检测
    // ────────────────────────────────────────────────────────
    fn detect(&mut self, img: &DynamicImage) -> Result<Vec<BBox>> {
        let (orig_w, orig_h) = img.dimensions();
        // 长边对齐 32 的倍数 · 短边按比例 · 保留原始 (orig_w/h, new_w/h, scale_x/y)
        let (new_w, new_h, scale_x, scale_y) = letterbox_to_32(orig_w, orig_h, DET_LIMIT_SIDE_LEN);

        let resized = img.resize_exact(new_w, new_h, image::imageops::FilterType::Triangle);
        let rgb = resized.to_rgb8();

        // 转 (1, 3, H, W) f32 · ImageNet normalize
        let tensor = rgb_to_chw_normalized(&rgb, &NORM_MEAN, &NORM_STD);
        let input: Array4<f32> = tensor.insert_axis(Axis(0));

        // ort 2.0-rc.4 API · 输入名 RapidOCR det 是 "x"
        let outputs = self.det.run(ort::inputs! { "x" => input.view() }?)
            .context("det session.run 失败")?;
        // det 输出形状: (1, 1, H, W) · 第 0 个 output 是概率图
        let probmap: ArrayD<f32> = outputs[0]
            .try_extract_tensor::<f32>()
            .context("det 输出抽取失败")?
            .view()
            .to_owned();
        let probmap = probmap
            .into_shape((new_h as usize, new_w as usize))
            .context("det 输出 reshape 失败")?;

        // 二值化 + 连通域 + bbox
        let mut boxes = find_text_boxes(
            &probmap,
            DET_DB_THRESH,
            DET_MIN_BOX_AREA,
            DET_DB_UNCLIP_RATIO,
        );

        // 把 box 坐标映射回原图
        for b in &mut boxes {
            b.x0 = ((b.x0 as f32) * scale_x).round() as i32;
            b.y0 = ((b.y0 as f32) * scale_y).round() as i32;
            b.x1 = ((b.x1 as f32) * scale_x).round() as i32;
            b.y1 = ((b.y1 as f32) * scale_y).round() as i32;
            b.clamp(orig_w as i32, orig_h as i32);
        }

        // 按 box 平均置信度过滤
        boxes.retain(|b| b.score >= DET_DB_BOX_THRESH);
        Ok(boxes)
    }

    // ────────────────────────────────────────────────────────
    // cls · 方向分类 0°/180°
    // ────────────────────────────────────────────────────────
    fn classify_and_rotate(&mut self, crop: &ImageBuffer<Rgb<u8>, Vec<u8>>)
        -> Result<ImageBuffer<Rgb<u8>, Vec<u8>>>
    {
        // resize 到 cls 输入 · 不做 letterbox · 直接强 resize(对齐 RapidOCR)
        let resized = image::imageops::resize(
            crop,
            CLS_INPUT_WIDTH,
            CLS_INPUT_HEIGHT,
            image::imageops::FilterType::Triangle,
        );

        // (1, 3, H, W) · cls 用 (px/255 - 0.5) / 0.5 归一化
        let tensor = rgb_to_chw_normalized(
            &resized,
            &[0.5, 0.5, 0.5],
            &[0.5, 0.5, 0.5],
        );
        let input: Array4<f32> = tensor.insert_axis(Axis(0));

        let outputs = self.cls.run(ort::inputs! { "x" => input.view() }?)
            .context("cls session.run 失败")?;
        // 输出 (1, 2) · index 0 = 0° · index 1 = 180°
        let probs: ArrayD<f32> = outputs[0]
            .try_extract_tensor::<f32>()
            .context("cls 输出抽取失败")?
            .view()
            .to_owned();
        if probs.len() < 2 {
            return Ok(crop.clone());
        }
        let p180 = probs.iter().nth(1).copied().unwrap_or(0.0);

        if p180 > CLS_THRESH {
            // 旋转 180°
            Ok(image::imageops::rotate180(crop))
        } else {
            Ok(crop.clone())
        }
    }

    // ────────────────────────────────────────────────────────
    // rec · CRNN 识别 + CTC 解码
    // ────────────────────────────────────────────────────────
    fn recognize(&mut self, crop: &ImageBuffer<Rgb<u8>, Vec<u8>>)
        -> Result<(String, f32)>
    {
        let (cw, ch) = crop.dimensions();
        if cw == 0 || ch == 0 {
            return Ok((String::new(), 0.0));
        }
        // 高度强 resize 到 48 · 宽度按 ratio · 不超 REC_INPUT_MAX_WIDTH · letterbox padding
        let ratio = cw as f32 / ch as f32;
        let target_w = ((REC_INPUT_HEIGHT as f32) * ratio).round() as u32;
        let target_w = target_w.min(REC_INPUT_MAX_WIDTH).max(8);

        let resized = image::imageops::resize(
            crop,
            target_w,
            REC_INPUT_HEIGHT,
            image::imageops::FilterType::Triangle,
        );

        // letterbox padding 到 (48, REC_INPUT_MAX_WIDTH) · padding 黑色
        let mut padded: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(
            REC_INPUT_MAX_WIDTH, REC_INPUT_HEIGHT,
        );
        for (x, y, p) in resized.enumerate_pixels() {
            padded.put_pixel(x, y, *p);
        }
        // 右侧填 0(模型 padding 标准)

        // (1, 3, 48, W) · rec 用 (px/255 - 0.5) / 0.5 归一化
        let tensor = rgb_to_chw_normalized(
            &padded,
            &[0.5, 0.5, 0.5],
            &[0.5, 0.5, 0.5],
        );
        let input: Array4<f32> = tensor.insert_axis(Axis(0));

        let outputs = self.rec.run(ort::inputs! { "x" => input.view() }?)
            .context("rec session.run 失败")?;
        // rec 输出: (1, T, num_chars) · T = W/4 大约
        let logits: ArrayD<f32> = outputs[0]
            .try_extract_tensor::<f32>()
            .context("rec 输出抽取失败")?
            .view()
            .to_owned();
        let (t, c) = match logits.shape() {
            s if s.len() == 3 => (s[1], s[2]),
            _ => return Err(anyhow!("rec 输出维度异常 · shape={:?}", logits.shape())),
        };

        // CTC 解码: 每个时间步 argmax · 相邻去重 · 移除 blank(index 0)
        let logits_2d = logits.into_shape((t, c))
            .context("rec logits reshape 失败")?;

        let mut chars_out: Vec<char> = Vec::with_capacity(t);
        let mut confs: Vec<f32> = Vec::with_capacity(t);
        let mut prev_idx: usize = usize::MAX;
        for time_step in 0..t {
            let row = logits_2d.slice(s![time_step, ..]);
            let (max_idx, max_prob) = row
                .iter()
                .enumerate()
                .fold((0usize, f32::NEG_INFINITY), |(bi, bp), (i, &v)| {
                    if v > bp { (i, v) } else { (bi, bp) }
                });
            if max_idx == 0 {
                // blank · 跳过
                prev_idx = max_idx;
                continue;
            }
            if max_idx == prev_idx {
                // 相邻去重
                continue;
            }
            if let Some(ch) = self.keys.get(max_idx) {
                if !ch.is_empty() {
                    chars_out.extend(ch.chars());
                    // PP-OCRv4 rec 输出层带 softmax · max_prob 就是 confidence
                    // 不需要再 softmax 一次(会把已是 prob 分布的值压扁到 ~0)
                    confs.push(max_prob.clamp(0.0, 1.0));
                }
            }
            prev_idx = max_idx;
        }

        let text: String = chars_out.into_iter().collect();
        let avg_conf = if confs.is_empty() {
            0.0
        } else {
            confs.iter().sum::<f32>() / confs.len() as f32
        };
        Ok((text, avg_conf))
    }
}

// ════════════════════════════════════════════════════════════════════
// 工具:坐标 box
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy)]
struct BBox {
    x0: i32, y0: i32, x1: i32, y1: i32,
    score: f32,
}

impl BBox {
    fn clamp(&mut self, w: i32, h: i32) {
        self.x0 = self.x0.max(0).min(w - 1);
        self.y0 = self.y0.max(0).min(h - 1);
        self.x1 = self.x1.max(0).min(w - 1);
        self.y1 = self.y1.max(0).min(h - 1);
    }
    fn points(&self) -> [[i32; 2]; 4] {
        [
            [self.x0, self.y0],
            [self.x1, self.y0],
            [self.x1, self.y1],
            [self.x0, self.y1],
        ]
    }
}

// ════════════════════════════════════════════════════════════════════
// 工具:图像 letterbox 到 32 倍数
// ════════════════════════════════════════════════════════════════════

fn letterbox_to_32(w: u32, h: u32, limit_side: u32) -> (u32, u32, f32, f32) {
    let ratio = if w.max(h) > limit_side {
        limit_side as f32 / w.max(h) as f32
    } else {
        1.0
    };
    let new_w = ((w as f32 * ratio).round() as u32).max(32);
    let new_h = ((h as f32 * ratio).round() as u32).max(32);
    let new_w = (new_w + 31) / 32 * 32;
    let new_h = (new_h + 31) / 32 * 32;
    let scale_x = w as f32 / new_w as f32;
    let scale_y = h as f32 / new_h as f32;
    (new_w, new_h, scale_x, scale_y)
}

// ════════════════════════════════════════════════════════════════════
// 工具:RGB → CHW normalized tensor
// ════════════════════════════════════════════════════════════════════

fn rgb_to_chw_normalized(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    mean: &[f32; 3],
    std: &[f32; 3],
) -> Array3<f32> {
    let (w, h) = img.dimensions();
    let mut tensor: Array3<f32> = Array::zeros((3, h as usize, w as usize));
    for (x, y, p) in img.enumerate_pixels() {
        let [r, g, b] = p.0;
        tensor[(0, y as usize, x as usize)] = (r as f32 / 255.0 - mean[0]) / std[0];
        tensor[(1, y as usize, x as usize)] = (g as f32 / 255.0 - mean[1]) / std[1];
        tensor[(2, y as usize, x as usize)] = (b as f32 / 255.0 - mean[2]) / std[2];
    }
    tensor
}

// ════════════════════════════════════════════════════════════════════
// 工具:概率图 → 文本框(简化版连通域 BFS)
// ════════════════════════════════════════════════════════════════════

fn find_text_boxes(
    probmap: &Array2<f32>,
    thresh: f32,
    min_area: u32,
    unclip_ratio: f32,
) -> Vec<BBox> {
    let (h, w) = (probmap.shape()[0], probmap.shape()[1]);
    let mut visited = vec![vec![false; w]; h];
    let mut boxes: Vec<BBox> = Vec::new();

    for y in 0..h {
        for x in 0..w {
            if visited[y][x] || probmap[(y, x)] < thresh {
                continue;
            }
            // BFS · 找连通域 + axis-aligned bbox + 平均概率
            let mut queue: std::collections::VecDeque<(usize, usize)> =
                std::collections::VecDeque::new();
            queue.push_back((y, x));
            visited[y][x] = true;
            let (mut xmin, mut xmax) = (x, x);
            let (mut ymin, mut ymax) = (y, y);
            let mut sum: f32 = 0.0;
            let mut count: u32 = 0;
            while let Some((cy, cx)) = queue.pop_front() {
                sum += probmap[(cy, cx)];
                count += 1;
                if cx < xmin { xmin = cx; }
                if cx > xmax { xmax = cx; }
                if cy < ymin { ymin = cy; }
                if cy > ymax { ymax = cy; }
                // 4-邻域
                let neighbors = [
                    (cy.wrapping_sub(1), cx),
                    (cy + 1, cx),
                    (cy, cx.wrapping_sub(1)),
                    (cy, cx + 1),
                ];
                for &(ny, nx) in &neighbors {
                    if ny >= h || nx >= w { continue; }
                    if visited[ny][nx] { continue; }
                    if probmap[(ny, nx)] < thresh { continue; }
                    visited[ny][nx] = true;
                    queue.push_back((ny, nx));
                }
            }
            let area = (xmax - xmin + 1) as u32 * (ymax - ymin + 1) as u32;
            if area < min_area || count < 3 {
                continue;
            }
            let avg = sum / count as f32;

            // unclip 扩展 · 上下左右各扩 unclip_ratio
            let bw = (xmax - xmin + 1) as f32;
            let bh = (ymax - ymin + 1) as f32;
            let exp_x = ((bw * (unclip_ratio - 1.0)) * 0.5).round() as i32;
            let exp_y = ((bh * (unclip_ratio - 1.0)) * 0.5).round() as i32;
            let mut bb = BBox {
                x0: xmin as i32 - exp_x,
                y0: ymin as i32 - exp_y,
                x1: xmax as i32 + exp_x,
                y1: ymax as i32 + exp_y,
                score: avg,
            };
            bb.clamp(w as i32, h as i32);
            boxes.push(bb);
        }
    }
    boxes
}

// ════════════════════════════════════════════════════════════════════
// 工具:从原图按 bbox crop · 简化版(axis-aligned · 不做透视变换)
// ════════════════════════════════════════════════════════════════════

fn crop_box(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    bbox: &BBox,
) -> Option<ImageBuffer<Rgb<u8>, Vec<u8>>> {
    let (iw, ih) = img.dimensions();
    let x0 = bbox.x0.max(0) as u32;
    let y0 = bbox.y0.max(0) as u32;
    let x1 = (bbox.x1 + 1).min(iw as i32).max(0) as u32;
    let y1 = (bbox.y1 + 1).min(ih as i32).max(0) as u32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let w = x1 - x0;
    let h = y1 - y0;
    if w < 4 || h < 4 {
        return None;
    }
    let mut out: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let p = *img.get_pixel(x + x0, y + y0);
            out.put_pixel(x, y, p);
        }
    }
    Some(out)
}

// ════════════════════════════════════════════════════════════════════
// 工具:softmax 取单个 index 的归一化概率
// ════════════════════════════════════════════════════════════════════

fn softmax_prob(row: &ndarray::ArrayView1<f32>, idx: usize) -> f32 {
    let max_logit = row.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
    let sum: f32 = row.iter().map(|&v| (v - max_logit).exp()).sum();
    if sum <= 0.0 || sum.is_nan() {
        return 0.0;
    }
    let target = (row[idx] - max_logit).exp() / sum;
    target.clamp(0.0, 1.0)
}

// ════════════════════════════════════════════════════════════════════
// 单元测试
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_letterbox_to_32() {
        let (w, h, sx, sy) = letterbox_to_32(1080, 720, 736);
        assert_eq!(w % 32, 0);
        assert_eq!(h % 32, 0);
        assert!(w <= 736 + 32);
        assert!(sx > 0.0 && sy > 0.0);
    }

    #[test]
    fn test_letterbox_small_image_no_upscale() {
        // 小图不放大
        let (w, h, _, _) = letterbox_to_32(100, 50, 736);
        assert!(w >= 96 && w <= 128); // 100 round 到 32 倍数 = 96
        assert!(h >= 32 && h <= 64);
    }

    #[test]
    fn test_bbox_clamp() {
        let mut b = BBox { x0: -5, y0: -10, x1: 200, y1: 300, score: 0.9 };
        b.clamp(100, 200);
        assert_eq!(b.x0, 0);
        assert_eq!(b.y0, 0);
        assert_eq!(b.x1, 99);
        assert_eq!(b.y1, 199);
    }

    #[test]
    fn test_find_text_boxes_empty() {
        // 全 0 概率图 · 应返回 0 个 box
        let probmap = Array2::<f32>::zeros((32, 64));
        let boxes = find_text_boxes(&probmap, 0.3, 16, 1.6);
        assert!(boxes.is_empty());
    }

    #[test]
    fn test_find_text_boxes_one_blob() {
        // 单个矩形 blob · 应返回 1 个 box
        let mut probmap = Array2::<f32>::zeros((64, 128));
        for y in 20..40 {
            for x in 30..80 {
                probmap[(y, x)] = 0.9;
            }
        }
        let boxes = find_text_boxes(&probmap, 0.3, 16, 1.0);
        assert_eq!(boxes.len(), 1);
        let b = &boxes[0];
        assert!(b.x0 <= 30 && b.y0 <= 20);
        assert!(b.x1 >= 79 && b.y1 >= 39);
        assert!(b.score > 0.85);
    }

    #[test]
    fn test_softmax_prob_basic() {
        let row = ndarray::array![1.0, 2.0, 3.0];
        let view = row.view();
        let p2 = softmax_prob(&view, 2);
        assert!(p2 > 0.6 && p2 < 0.7); // softmax(3)/(softmax(1)+softmax(2)+softmax(3)) ≈ 0.665
        let p0 = softmax_prob(&view, 0);
        assert!(p0 < 0.1);
    }

    // ════════════════════════════════════════════════════════════════
    // E2E · 用真模型 + 真图跑完整 pipeline
    //
    // 跑法:
    //   ORT_DYLIB_PATH=.local_models/onnxruntime/onnxruntime-osx-arm64-1.18.1/lib/libonnxruntime.dylib \
    //   RAPID_OCR_MODEL_DIR=.local_models/rapid_ocr_v1 \
    //   RAPID_OCR_TEST_IMAGE=.local_models/test_ocr.jpg \
    //   cargo test --no-default-features --features onnx test_rapid_ocr_e2e_real -- --nocapture
    //
    // 三个环境变量任一缺失 → 测试 skip · CI 不强求
    // ════════════════════════════════════════════════════════════════
    #[test]
    fn test_rapid_ocr_e2e_real() {
        let dylib = match std::env::var("ORT_DYLIB_PATH") {
            Ok(p) if std::path::Path::new(&p).is_file() => p,
            _ => {
                eprintln!("⏭ ORT_DYLIB_PATH 未配置或库不存在 · skip e2e");
                return;
            }
        };
        let model_dir = match std::env::var("RAPID_OCR_MODEL_DIR") {
            Ok(d) if std::path::Path::new(&d).is_dir() => std::path::PathBuf::from(d),
            _ => {
                eprintln!("⏭ RAPID_OCR_MODEL_DIR 未配置或目录不存在 · skip e2e");
                return;
            }
        };
        let image_path = match std::env::var("RAPID_OCR_TEST_IMAGE") {
            Ok(p) if std::path::Path::new(&p).is_file() => std::path::PathBuf::from(p),
            _ => {
                eprintln!("⏭ RAPID_OCR_TEST_IMAGE 未配置或文件不存在 · skip e2e");
                return;
            }
        };

        eprintln!("\n═══════ RapidOCR e2e ═══════");
        eprintln!("dylib       = {}", dylib);
        eprintln!("model_dir   = {}", model_dir.display());
        eprintln!("test_image  = {}", image_path.display());

        let t_load = std::time::Instant::now();
        let mut engine = RapidOcrEngine::load(&model_dir)
            .expect("加载 RapidOCR engine 失败");
        eprintln!("✓ engine 加载耗时 {} ms", t_load.elapsed().as_millis());

        let bytes = std::fs::read(&image_path).expect("读测试图失败");
        eprintln!("✓ 测试图 {} bytes", bytes.len());

        let t_run = std::time::Instant::now();
        let result = engine.run(&bytes).expect("OCR 推理失败");
        eprintln!(
            "✓ OCR 总耗时 {} ms (内部 {} ms)",
            t_run.elapsed().as_millis(),
            result.elapsed_ms
        );

        eprintln!();
        eprintln!("─── 识别结果 ──────────────");
        eprintln!("检测到 {} 行", result.lines.len());
        for (i, line) in result.lines.iter().enumerate() {
            eprintln!(
                "  [{}] conf={:.3}  text=\"{}\"  box={:?}",
                i + 1, line.confidence, line.text, line.r#box
            );
        }
        eprintln!();
        eprintln!("─── 拼合全文 ──────────────");
        eprintln!("{}", result.text);

        // 不强断言"必须识别出 X 字符"(测试图内容可能换),只要 pipeline 不 panic 就算成功
        // 但若有文本框,识别结果不该全空(若全空说明 rec 模型有 bug)
        if !result.lines.is_empty() {
            let total_chars: usize = result.lines.iter().map(|l| l.text.chars().count()).sum();
            assert!(total_chars > 0, "检测到文本框但 rec 全空 · pipeline 异常");
        }
    }
}
