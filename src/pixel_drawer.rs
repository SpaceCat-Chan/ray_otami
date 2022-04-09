use std::{
    collections::HashMap,
    ops::DerefMut,
    sync::{Arc, Mutex},
    thread::spawn,
};

use cgmath::prelude::*;
use rand_distr::Distribution;
use rayon::iter::{ParallelBridge, ParallelIterator};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Material {
    pub color: cgmath::Vector3<f64>,
    pub emitance: cgmath::Vector3<f64>,
    pub metalness: f64,
    pub roughness: f64,
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

impl Object {
    fn estimate_distance(&self, point: cgmath::Point3<f64>) -> f64 {
        match self {
            Self::Sphere { center, radius, .. } => point.distance(*center) - radius,
            Self::Box {
                lower_corner,
                upper_corner,
                ..
            } => {
                let center = lower_corner.midpoint(*upper_corner);
                let b = center - lower_corner;

                let q = (point - center).map(|x| x.abs()) - b;
                q.map(|x| x.max(0.0)).distance(cgmath::vec3(0.0, 0.0, 0.0))
                    + q.x.max(q.y.max(q.z)).min(0.0)
            }
            Self::PosModulo(o, period) => o.estimate_distance(point.map(|x| x.rem_euclid(*period))),
            Self::Inv(o) => -o.estimate_distance(point),
            Self::Max(a, b) => a.estimate_distance(point).max(b.estimate_distance(point)),
            Self::Min(a, b) => a.estimate_distance(point).min(b.estimate_distance(point)),
            Self::Torus {
                major_radius,
                minor_radius,
                center,
                ..
            } => {
                let mut point = center - point;
                let mut move_by = point;
                move_by.y = 0.0;
                //if move_by == cgmath::vec3(0.0, 0.0, 0.0) {
                //    move_by = cgmath::vec3(1.0, 0.0, 1.0);
                //}
                let move_by = move_by.normalize_to(*major_radius);
                point -= move_by;

                point.magnitude() - minor_radius
            }
        }
    }

    fn get_metadata(&self, point: cgmath::Point3<f64>) -> (f64, &str) {
        match self {
            Self::Sphere { material, .. } => (self.estimate_distance(point), material),
            Self::Box { material, .. } => (self.estimate_distance(point), material),
            Self::PosModulo(o, period) => o.get_metadata(point.map(|x| x.rem_euclid(*period))),
            Self::Inv(o) => {
                let (dist, meta) = o.get_metadata(point);
                (-dist, meta)
            }
            Object::Min(a, b) => {
                let (a_dist, a_meta) = a.get_metadata(point);
                let (b_dist, b_meta) = b.get_metadata(point);
                if a_dist < b_dist {
                    (a_dist, a_meta)
                } else {
                    (b_dist, b_meta)
                }
            }
            Object::Max(a, b) => {
                let (a_dist, a_meta) = a.get_metadata(point);
                let (b_dist, b_meta) = b.get_metadata(point);
                if a_dist > b_dist {
                    (a_dist, a_meta)
                } else {
                    (b_dist, b_meta)
                }
            }
            Self::Torus { material, .. } => (self.estimate_distance(point), material),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct World {
    pub max_ray_depth: u32,
    pub sky_color: cgmath::Vector3<f64>,
    pub objects: Vec<Object>,
    pub materials: HashMap<String, Material>,
}

static BLACK: cgmath::Vector3<f64> = cgmath::vec3(0.0, 0.0, 0.0);
static BLACK_MATERIAL: Material = Material {
    color: BLACK,
    emitance: BLACK,
    metalness: 0.0,
    roughness: 1.0,
};

impl World {
    fn estimate_distance(&self, point: cgmath::Point3<f64>) -> f64 {
        self.objects
            .iter()
            .map(|x| x.estimate_distance(point))
            .reduce(f64::min)
            .unwrap_or(0.0)
    }

    fn get_closest_metadata(&self, point: cgmath::Point3<f64>) -> &Material {
        self.objects
            .iter()
            .map(|x| x.get_metadata(point))
            .reduce(|acc, x| if x.0 < acc.0 { x } else { acc })
            .and_then(|(_, a)| self.materials.get(a))
            .unwrap_or(&BLACK_MATERIAL)
    }

    fn get_distance_gradient(&self, point: cgmath::Point3<f64>) -> cgmath::Vector3<f64> {
        let x_neg = self.estimate_distance(point + cgmath::vec3(-0.005, 0.0, 0.0));
        let x_pos = self.estimate_distance(point + cgmath::vec3(0.005, 0.0, 0.0));
        let y_neg = self.estimate_distance(point + cgmath::vec3(0.0, -0.005, 0.0));
        let y_pos = self.estimate_distance(point + cgmath::vec3(0.0, 0.005, 0.0));
        let z_neg = self.estimate_distance(point + cgmath::vec3(0.0, 0.0, -0.005));
        let z_pos = self.estimate_distance(point + cgmath::vec3(0.0, 0.0, 0.005));
        cgmath::vec3(x_pos - x_neg, y_pos - y_neg, z_pos - z_neg)
    }
}

struct HitResult {
    position: cgmath::Point3<f64>,
    previous_position: cgmath::Point3<f64>,
    hit_anything: bool,
}

fn cast_ray(
    from: cgmath::Point3<f64>,
    direction: cgmath::Vector3<f64>,
    world: &World,
) -> HitResult {
    let mut position = from;
    let mut prev_pos = from;
    for _ in 0..1000 {
        let current_distance = world.estimate_distance(position);
        if current_distance < 0.0001 {
            return HitResult {
                position,
                previous_position: prev_pos,
                hit_anything: true,
            };
        }
        if current_distance > 10000.0 {
            return HitResult {
                position,
                previous_position: prev_pos,
                hit_anything: false,
            };
        }
        prev_pos = position;
        position += direction * current_distance;
    }
    HitResult {
        position,
        previous_position: prev_pos,
        hit_anything: false,
    }
}

fn distribution_ggx(
    normal: cgmath::Vector3<f64>,
    halfway: cgmath::Vector3<f64>,
    roughness: f64,
) -> f64 {
    let roughness2 = roughness.powi(4);
    let ndot = normal.dot(halfway).max(0.0);
    let denom = (ndot * ndot) * (roughness2 - 1.0) + 1.0;

    roughness2 / (std::f64::consts::PI * denom * denom)
}

//returns the smallest and largest possible values for a given roughness
fn distribution_ggx_limits(roughness: f64) -> (f64, f64) {
    let roughness2 = roughness.powi(4);

    (roughness2, roughness2 / (roughness2 * roughness2))
}

fn inverse_distribution_ggx(roughness: f64, value: f64) -> f64 {
    let roughness2 = roughness.powi(4);

    let denom = roughness2 * roughness2 * value - 2.0 * value * roughness2 + value;

    let num1 = value - roughness2 * value;
    let num2 = ((roughness2 - 1.0) * (roughness2 - 1.0) * roughness2 * value).sqrt();

    let ans1 = (num1 - num2) / denom;
    let ans2 = (num1 + num2) / denom;

    if ans1 < 0.0 {
        ans2
    } else {
        ans1
    }
}

fn geometry_schlick_ggx(normal_dot_dir: f64, mapped_roughness: f64) -> f64 {
    normal_dot_dir / (normal_dot_dir * (1.0 - mapped_roughness) + mapped_roughness)
}

fn geometry_smith(
    normal: cgmath::Vector3<f64>,
    view: cgmath::Vector3<f64>,
    light: cgmath::Vector3<f64>,
    roughness: f64,
) -> f64 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    geometry_schlick_ggx(normal.dot(view).max(0.0), k)
        * geometry_schlick_ggx(normal.dot(light).max(0.0), k)
}

fn fresnel_schlick(cos_theta: f64, f0: cgmath::Vector3<f64>) -> cgmath::Vector3<f64> {
    f0 + f0.map(|v| 1.0 - v) * (1.0 - cos_theta).clamp(0.0, 1.0).powi(5)
}

fn select_direction<T: rand::Rng>(
    view_dir: cgmath::Vector3<f64>,
    roughness: f64,
    metalness: f64,
    mut rand: T,
) -> (cgmath::Vector3<f64>, cgmath::Vector3<f64>, f64) {
    if rand_distr::Bernoulli::new(1.0 - metalness)
        .unwrap()
        .sample(&mut rand)
    {
        // not metallic enough for the complex method!
        let dist = rand_distr::UnitSphere;
        let ray_dir: [f64; 3] = dist.sample(&mut rand);
        let ray_dir = cgmath::vec3(ray_dir[0], ray_dir[1].abs(), ray_dir[2]);
        let halfway = (ray_dir + view_dir).normalize();
        return (
            ray_dir,
            halfway,
            distribution_ggx(cgmath::vec3(0.0, 1.0, 0.0), halfway, roughness),
        );
    }
    let (min_ndf, max_ndf) = distribution_ggx_limits(roughness);

    if max_ndf.is_nan() {
        //roughness is 0, the halfway vector must be lined up exactly
        let normal = cgmath::vec3(0.0, 1.0, 0.0);
        return (
            (-view_dir) - (normal.dot(-view_dir) * 2.0 * normal),
            normal,
            1.0,
        );
    }

    let num = rand::distributions::Uniform::new_inclusive(0.0, 1.0).sample(&mut rand);
    let num = num * num;
    let num = num * (max_ndf - min_ndf) + min_ndf;

    let cos_angle = inverse_distribution_ggx(roughness, num);

    if cos_angle.is_nan() {
        let dist = rand_distr::UnitSphere;
        // roughness is exactly 1, which means the direction must not be biased
        let ray_dir: [f64; 3] = dist.sample(&mut rand);
        let ray_dir = cgmath::vec3(ray_dir[0], ray_dir[1].abs(), ray_dir[2]);
        return (ray_dir, (ray_dir + view_dir).normalize(), 1.0);
    }

    let theta =
        rand::distributions::Uniform::new(0.0, 2.0 * std::f64::consts::PI).sample(&mut rand);

    let sin_angle = (1.0 - cos_angle * cos_angle).sqrt();

    let (x, z) = theta.sin_cos();
    let (x, z) = (x * sin_angle, z * sin_angle);

    let halfway = cgmath::vec3(x, cos_angle, z).normalize();

    (
        (-view_dir) - (halfway.dot(-view_dir) * 2.0 * halfway),
        halfway,
        1.0,
    )
}

pub fn render_ray(
    from: cgmath::Point3<f64>,
    direction: cgmath::Vector3<f64>,
    world: &World,
    depth: u32,
) -> cgmath::Vector3<f64> {
    let ray = cast_ray(from, direction, world);
    if ray.hit_anything {
        let metadata = world.get_closest_metadata(ray.position);
        if depth == world.max_ray_depth {
            return metadata.emitance;
        }
        //send a new ray, get diffuse and specular weight, do math
        let normal = world.get_distance_gradient(ray.position).normalize();
        let rotation = cgmath::Basis3::between_vectors(cgmath::vec3(0.0, 1.0, 0.0), normal);

        let rand = rand::thread_rng();

        // experiment: bias the ray selection so directions more likely so influence the final color are more likely to be chosen
        let (ray_dir, halfway, ndf) = select_direction(
            rotation.invert().rotate_vector(-direction),
            metadata.roughness,
            metadata.metalness,
            rand,
        );
        let (ray_dir, halfway) = (
            rotation.rotate_vector(ray_dir).normalize(),
            rotation.rotate_vector(halfway),
        );

        let ray_color = render_ray(ray.previous_position, ray_dir, world, depth + 1);
        let f0 = cgmath::vec3(0.04, 0.04, 0.04);
        let f0 = f0.lerp(metadata.color, metadata.metalness);
        let f = fresnel_schlick(normal.dot(halfway).max(0.0), f0);
        let g = geometry_smith(normal, -direction, ray_dir, metadata.roughness);
        let specular = (g * f * ndf)
            / (4.0 * normal.dot(-direction).max(0.0) * normal.dot(ray_dir).max(0.0) + 0.000001);
        let k_d = f.map(|x| 1.0 - x);
        let k_d = k_d * (1.0 - metadata.metalness);
        (k_d.mul_element_wise(metadata.color) / std::f64::consts::PI + specular)
            .mul_element_wise(ray_color)
            * normal.dot(ray_dir).max(0.0)
            * 10.0
            + metadata.emitance
    } else {
        BLACK
    }
}

pub fn render_pixel(
    (width, height): (u32, u32),
    pixel_idx: u32,
    world: &World,
) -> (f64, f64, f64, f64) {
    let pixel_pos = (pixel_idx % width, pixel_idx / width);
    let pixel_pos = (
        (pixel_pos.0 as f64 / width as f64 - 0.5) * 2.0,
        (pixel_pos.1 as f64 / height as f64 - 0.5) * 2.0,
    );

    let color = render_ray(
        cgmath::point3(0.0, 0.0, 0.0),
        cgmath::vec3(pixel_pos.0, pixel_pos.1, 1.0).normalize(),
        world,
        0,
    );
    //color.div_assign_element_wise(color.map(|x| x + 1.0));
    (color.x, color.y, color.z, 1.0)
}

pub fn render_to_buffer(buffer: Arc<Mutex<Vec<u8>>>, (width, height): (u32, u32), world: &World) {
    let (mut sender, mut reciever) = futures::channel::mpsc::unbounded::<(usize, [f64; 4])>();
    let reciever = spawn(move || {
        let mut ray_count = vec![0usize; (width * height) as usize];
        let mut actual_buffer = vec![0f64; (width * height * 4) as usize];
        'outer: loop {
            if let Ok(mut lock) = buffer.lock() {
                let r = reciever.try_next();
                match r {
                    Ok(Some((index, val))) => {
                        ray_count[index] += 1;
                        let ray_count = ray_count[index] as f64;
                        for (n, item) in val.iter().enumerate() {
                            let old_val = actual_buffer[index * 4 + n];
                            let new_val = (item + old_val * (ray_count - 1.0)) / ray_count;
                            actual_buffer[index * 4 + n] = new_val;
                            let new_val = new_val / (new_val + 1.0);
                            lock.deref_mut()[index * 4 + n] = (new_val * 255.0) as u8;
                        }
                    }
                    Ok(None) => break 'outer,
                    Err(_) => continue,
                }
            }
        }
    });
    (0..)
        .into_iter()
        .par_bridge()
        .map(|p| p % (width * height))
        .map(|pos| (pos, render_pixel((width, height), pos, world)))
        .map(|(pos, (b, g, r, a))| (pos as usize, [r, g, b, a]))
        .for_each(|a| sender.unbounded_send(a).unwrap());
    sender.disconnect();
    reciever.join().unwrap();
}
