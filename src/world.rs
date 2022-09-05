use cgmath::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct SimpleRotation {
    pub from: cgmath::Vector3<f64>,
    pub to: cgmath::Vector3<f64>,
}

impl Default for SimpleRotation {
    fn default() -> Self {
        Self {
            from: cgmath::vec3(1.0, 0.0, 0.0),
            to: cgmath::vec3(1.0, 0.0, 0.0),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct Material {
    #[serde(default = "cgmath::Vector3::<f64>::zero")]
    pub color: cgmath::Vector3<f64>,
    #[serde(default = "cgmath::Vector3::<f64>::zero")]
    pub emitance: cgmath::Vector3<f64>,
    #[serde(default)]
    pub metalness: f64,
    #[serde(default)]
    pub roughness: f64,
    #[serde(default)]
    pub is_portal: bool,
    #[serde(default = "cgmath::Vector3::<f64>::zero")]
    pub translation: cgmath::Vector3<f64>,
    #[serde(default = "cgmath::Vector3::<f64>::zero")]
    pub rotate_around: cgmath::Vector3<f64>,
    #[serde(default)]
    pub rotation: SimpleRotation,
}

#[derive(Serialize, Deserialize)]
pub enum Object {
    Sphere {
        center: cgmath::Point3<f64>,
        radius: f64,
        material: String,
    },
    Box {
        lower_corner: cgmath::Point3<f64>,
        upper_corner: cgmath::Point3<f64>,
        material: String,
    },
    PosModulo(Box<Object>, f64),
    Inv(Box<Object>),
    Min(Box<Object>, Box<Object>),
    Max(Box<Object>, Box<Object>),
    Torus {
        major_radius: f64,
        minor_radius: f64,
        center: cgmath::Point3<f64>,
        material: String,
    },
}

#[derive(Serialize, Deserialize)]
pub struct World {
    pub max_ray_depth: u32,
    pub sky_color: cgmath::Vector3<f64>,
    pub objects: Vec<Object>,
    pub materials: HashMap<String, Material>,
}
