use image::imageops::{crop_imm, resize, FilterType};
use image::{ImageFormat, Rgb, RgbImage};
use imgseek::{run_index, run_query, IndexArgs, QueryArgs};
use imgseek_core::Config;
use std::fs;
use std::path::{Path, PathBuf};

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    [
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

fn generate_distinct_image(idx: usize, width: u32, height: u32) -> RgbImage {
    let mut img = RgbImage::new(width, height);
    let hue = (idx as f32 * (360.0 / 20.0)) % 360.0;

    // Background vertical gradient
    for y in 0..height {
        let sat = (0.6 + 0.35 * ((y as f32 * 0.05).sin())).clamp(0.0, 1.0);
        let val = (0.4 + 0.55 * (y as f32 / height as f32)).clamp(0.0, 1.0);
        let bg = Rgb(hsv_to_rgb(hue, sat, val));
        for x in 0..width {
            img.put_pixel(x, y, bg);
        }
    }

    // Foreground shape
    let cx = (40 + (idx * 37) % 80) as i32;
    let cy = (40 + (idx * 53) % 80) as i32;
    let r = (20 + (idx * 7) % 25) as i32;
    let fg = Rgb(hsv_to_rgb((hue + 180.0) % 360.0, 0.9, 0.95));

    match idx % 3 {
        0 => {
            // Circle
            for y in (cy - r)..=(cy + r) {
                for x in (cx - r)..=(cx + r) {
                    if (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r
                        && x >= 0
                        && x < width as i32
                        && y >= 0
                        && y < height as i32
                    {
                        img.put_pixel(x as u32, y as u32, fg);
                    }
                }
            }
        }
        1 => {
            // Rectangle
            for y in (cy - r)..=(cy + r) {
                for x in (cx - r)..=(cx + r) {
                    if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
                        img.put_pixel(x as u32, y as u32, fg);
                    }
                }
            }
        }
        _ => {
            // Triangle with vertices (cx, cy-r), (cx-r, cy+r), (cx+r, cy+r)
            for y in (cy - r)..=(cy + r) {
                let progress = (y - (cy - r)) as f32 / (2 * r) as f32;
                let half_span = (r as f32 * progress).round() as i32;
                for x in (cx - half_span)..=(cx + half_span) {
                    if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
                        img.put_pixel(x as u32, y as u32, fg);
                    }
                }
            }
        }
    }

    img
}

fn apply_crop(img: &RgbImage) -> RgbImage {
    let (w, h) = img.dimensions();
    let crop_x = 5;
    let crop_y = 5;
    let crop_w = w - 2 * crop_x;
    let crop_h = h - 2 * crop_y;
    crop_imm(img, crop_x, crop_y, crop_w, crop_h).to_image()
}

fn apply_resize(img: &RgbImage) -> RgbImage {
    resize(img, 100, 100, FilterType::Triangle)
}

fn apply_brightness(img: &RgbImage, delta: i16) -> RgbImage {
    let mut out = img.clone();
    for pixel in out.pixels_mut() {
        for c in 0..3 {
            let val = pixel[c] as i16 + delta;
            pixel[c] = val.clamp(0, 255) as u8;
        }
    }
    out
}

fn apply_watermark(img: &RgbImage) -> RgbImage {
    let mut out = img.clone();
    let (w, h) = out.dimensions();
    let x_start = w.saturating_sub(20);
    let x_end = w.saturating_sub(5);
    let y_start = h.saturating_sub(20);
    let y_end = h.saturating_sub(5);
    for x in x_start..x_end {
        for y in y_start..y_end {
            out.put_pixel(x, y, Rgb([255, 255, 255]));
        }
    }
    out
}

type PerturbFn = Box<dyn Fn(&RgbImage, &Path) -> PathBuf>;

#[test]
fn test_query_perturbations_rank_one() {
    let test_id = format!(
        "imgseek_perturb_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let temp_root = std::env::temp_dir().join(test_id);
    let images_dir = temp_root.join("corpus");
    let query_dir = temp_root.join("queries");
    let index_dir = temp_root.join("index");
    let blob_dir = temp_root.join("blobs");

    fs::create_dir_all(&images_dir).expect("create images dir");
    fs::create_dir_all(&query_dir).expect("create query dir");
    fs::create_dir_all(&index_dir).expect("create index dir");
    fs::create_dir_all(&blob_dir).expect("create blob dir");

    let num_images = 20;
    let mut indexed_filenames = Vec::new();

    for i in 0..num_images {
        let filename = format!("img_{:02}.png", i);
        let dst_path = images_dir.join(&filename);
        let img = generate_distinct_image(i, 160, 160);
        img.save(&dst_path)
            .unwrap_or_else(|e| panic!("save {:?}: {:?}", dst_path, e));
        indexed_filenames.push(filename);
    }

    let mut config = Config::default();
    config.index.dir = index_dir;
    config.index.blob_dir = blob_dir;

    let index_args = IndexArgs {
        subcommand: None,
        paths: vec![images_dir.clone()],
        corpus: Some("local:test".to_string()),
        recursive: false,
        follow_symlinks: false,
        workers: Some(2),
        batch_size: 20,
        force_reindex: false,
        verbose: false,
    };
    run_index(&index_args, &config).expect("index 20 images");

    let perturbations: Vec<(&str, PerturbFn)> = vec![
        (
            "crop",
            Box::new(|img, dir| {
                let p = dir.join("crop.png");
                let cropped = apply_crop(img);
                cropped.save(&p).expect("save cropped");
                p
            }),
        ),
        (
            "resize",
            Box::new(|img, dir| {
                let p = dir.join("resize.png");
                let resized = apply_resize(img);
                resized.save(&p).expect("save resized");
                p
            }),
        ),
        (
            "reencode_jpeg",
            Box::new(|img, dir| {
                let p = dir.join("reencoded.jpg");
                img.save_with_format(&p, ImageFormat::Jpeg)
                    .expect("save jpeg");
                p
            }),
        ),
        (
            "brightness",
            Box::new(|img, dir| {
                let p = dir.join("brightness.png");
                let bright = apply_brightness(img, 20);
                bright.save(&p).expect("save brightness");
                p
            }),
        ),
        (
            "watermark",
            Box::new(|img, dir| {
                let p = dir.join("watermark.png");
                let wm = apply_watermark(img);
                wm.save(&p).expect("save watermark");
                p
            }),
        ),
    ];

    let mut total_tested = 0;
    for (i, expected_file) in indexed_filenames.iter().enumerate() {
        let orig_img_path = images_dir.join(expected_file);
        let dyn_img = image::open(&orig_img_path).expect("open original");
        let rgb_img = dyn_img.to_rgb8();

        for (perturb_name, perturb_fn) in &perturbations {
            let perturb_sub_dir = query_dir.join(format!("{}_{}", i, perturb_name));
            fs::create_dir_all(&perturb_sub_dir).expect("create perturb dir");

            let query_img_path = perturb_fn(&rgb_img, &perturb_sub_dir);

            let query_args = QueryArgs {
                image_path: query_img_path,
                limit: 5,
                min_score: Some(0.0),
                json: false,
                explain: false,
                corpus: None,
            };

            let results = run_query(&query_args, &config).unwrap_or_else(|e| {
                panic!(
                    "Query failed for image {} with perturbation {}: {:?}",
                    expected_file, perturb_name, e
                )
            });

            assert!(
                !results.is_empty(),
                "No results for image {} with perturbation {}",
                expected_file,
                perturb_name
            );

            let top = &results[0];
            assert_eq!(
                top.rank, 1,
                "Top match should have rank 1 for {} ({})",
                expected_file, perturb_name
            );

            assert!(
                top.origin_uri.ends_with(expected_file),
                "Top match should be {} for perturbation {}, but got {} (score: {:.4})",
                expected_file,
                perturb_name,
                top.origin_uri,
                top.score
            );

            assert!(
                top.score >= 0.55,
                "Score for {} ({}) was {:.4}, expected >= 0.55",
                expected_file,
                perturb_name,
                top.score
            );

            total_tested += 1;
        }
    }

    assert_eq!(
        total_tested,
        num_images * perturbations.len(),
        "All 100 perturbed image queries should have executed"
    );

    // Clean up
    let _ = fs::remove_dir_all(&temp_root);
}
