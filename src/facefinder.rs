use std::{sync::Mutex, borrow::BorrowMut};

use once_cell::sync::Lazy;
use pico_detect::{Detector, Shaper, Localizer, MultiScale, Detection, nalgebra::{Point2, Similarity2, Isometry2}};
use rand::SeedableRng;
use rand_xorshift::XorShiftRng;
use serde::{Serialize, Deserialize};
use anyhow::{anyhow, Result};

pub static FACEFINDER: Lazy<Mutex<(Detector, Localizer, Shaper)>> = Lazy::new(||{
    //加载模型
    let facefinder_bin = include_bytes!("../models/facefinder").to_vec();
    let puploc_bin = include_bytes!("../models/puploc.bin").to_vec();
    let shaper_bin = include_bytes!("../models/shaper_5_face_landmarks.bin").to_vec();

    let facefinder = Detector::from_readable(facefinder_bin.as_slice()).unwrap();
    let puploc = Localizer::from_readable(puploc_bin.as_slice()).unwrap();
    let shaper = Shaper::from_readable(shaper_bin.as_slice()).unwrap();
    Mutex::new((facefinder, puploc, shaper))
});

#[derive(Serialize, Deserialize, Debug)]
pub struct Opt {
    pub min_size: u32,
    pub scale_factor: f32,
    pub shift_factor: f32,
    /// `threshold` -- if IoU is bigger then a detection is a part of a cluster.
    pub threshold: f32,
}

impl Default for Opt{
    fn default() -> Self {
        Opt {
            min_size: 100,
            shift_factor: 0.1,
            scale_factor: 1.1,
            threshold: 0.2
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Rect {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

pub type Point = [f32; 2];

#[derive(Serialize, Deserialize, Debug)]
pub struct Face {
    score: f32,
    rect: Rect,
    shape: Vec<Point>,
    pupils: (Point, Point),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PostData{
    pub img: String,
    pub min_size: Option<u32>,
    pub shift_factor: Option<f32>,
    pub scale_factor: Option<f32>,
    pub threshold: Option<f32>
}

pub fn detect_faces(opt: &Opt, img: &str) -> Result<Vec<Face>> {
    let mut facefinder = FACEFINDER.lock()
        .map_err(|err| anyhow!("{:?}", err) )?;

    let facefinder = facefinder.borrow_mut();

    let image_buf = base64::decode(img)?;
    let gray = image::load_from_memory(&image_buf)?.into_luma8();

    // initialize multiscale
    let multiscale = MultiScale::default()
        .with_size_range(opt.min_size, gray.width())
        .with_shift_factor(opt.shift_factor)
        .with_scale_factor(opt.scale_factor);

    // source of "randomness" for perturbated search for pupil
    let mut rng = XorShiftRng::seed_from_u64(42u64);
    let nperturbs = 31usize;

    Ok(Detection::clusterize(multiscale.run(&facefinder.0, &gray).as_mut(), opt.threshold)
        .iter()
        .filter_map(|detection| {
            if detection.score() < 40.0 {
                return None;
            }

            let (center, size) = (detection.center(), detection.size());
            let rect = pico_detect::Rect::at(
                (center.x - size / 2.0) as i32,
                (center.y - size / 2.0) as i32,
            )
            .of_size(size as u32, size as u32);

            let shape = facefinder.2.predict(&gray, rect);
            let pupils = Shape5::find_eyes_roi(&shape);
            let pupils = (
                facefinder.1.perturb_localize(&gray, pupils.0, &mut rng, nperturbs),
                facefinder.1.perturb_localize(&gray, pupils.1, &mut rng, nperturbs),
            );

            Some(Face {
                rect: Rect { left: rect.left(), top: rect.top(), width: rect.width(), height: rect.height() },
                score: detection.score(),
                shape: shape.iter().map(|p| [p[0], p[1]]).collect(),
                pupils: ( [pupils.0[0], pupils.0[1]], [pupils.1[0], pupils.1[1]] ),
            })
        })
        .collect::<Vec<Face>>())
}

enum Shape5 {
    LeftOuterEyeCorner = 0,
    LeftInnerEyeCorner = 1,
    RightOuterEyeCorner = 2,
    RightInnerEyeCorner = 3,
    #[allow(dead_code)]
    Nose = 4,
}

impl Shape5 {
    fn size() -> usize {
        5
    }

    #[allow(dead_code)]
    fn find_eye_centers(shape: &[nalgebra::Point2<f32>]) -> (nalgebra::Point2<f32>, nalgebra::Point2<f32>) {
        assert_eq!(shape.len(), Self::size());
        (
            nalgebra::center(
                &shape[Self::LeftInnerEyeCorner as usize],
                &shape[Self::LeftOuterEyeCorner as usize],
            ),
            nalgebra::center(
                &shape[Self::RightInnerEyeCorner as usize],
                &shape[Self::RightOuterEyeCorner as usize],
            ),
        )
    }

    fn find_eyes_roi(shape: &[Point2<f32>]) -> (Similarity2<f32>, Similarity2<f32>) {
        assert_eq!(shape.len(), Self::size());
        let (li, lo) = (
            &shape[Self::LeftInnerEyeCorner as usize],
            &shape[Self::LeftOuterEyeCorner as usize],
        );
        let (ri, ro) = (
            &shape[Self::RightInnerEyeCorner as usize],
            &shape[Self::RightOuterEyeCorner as usize],
        );

        let (dl, dr) = (lo - li, ri - ro);
        let (l, r) = (li + dl.scale(0.5), ro + dr.scale(0.5));

        (
            Similarity2::from_isometry(Isometry2::translation(l.x, l.y), dl.norm() * 1.1),
            Similarity2::from_isometry(Isometry2::translation(r.x, r.y), dr.norm() * 1.1),
        )
    }
}