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
    Cylinder {
        center: cgmath::Point3<f64>,
        height: f64,
        radius: f64,
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
    pub camera: Camera,
}

fn default_up() -> cgmath::Vector3<f64> {
    cgmath::vec3(0.0, -1.0, 0.0)
}

fn default_fov() -> f64 {
    90.0
}

#[derive(Serialize, Deserialize)]
pub struct Camera {
    pub position: cgmath::Vector3<f64>,
    pub look_direction: cgmath::Vector3<f64>,
    #[serde(default = "default_up")]
    pub up_direction: cgmath::Vector3<f64>,
    #[serde(default = "default_fov")]
    pub fov_y: f64,
    #[serde(default = "default_fov")]
    pub fov_x: f64,
}

struct IdentGen {
    count: usize,
}

impl IdentGen {
    fn gen(&mut self, name: &str) -> String {
        let a = self.count;
        self.count += 1;
        format!("{}_{}", name, a)
    }
}

impl World {
    pub fn create_shader_function(&self) -> (String, Vec<Material>) {
        let mut materials = Vec::new();

        let material_name_to_index = self
            .materials
            .iter()
            .enumerate()
            .inspect(|(_, (_, material))| {
                materials.push(**material);
            })
            .map(|(index, (name, _))| (name.clone(), index))
            .collect::<HashMap<_, _>>();

        let mut ident_gen = IdentGen { count: 0 };

        let mut object_sdf_results = vec![];
        for object in &self.objects {
            object_sdf_results
                .push(object.create_shader_form(&mut ident_gen, &material_name_to_index))
        }

        let object_sdfs = match object_sdf_results.split_first() {
            None => "//undefined behaviour or something lol".to_owned(),
            Some(((first_string, first_mat, first_dist), rest)) => {
                let mut string = format!(
                    "
                    {first_string}
                    float running_lowest_distance = {first_dist};
                    uint running_lowest_mat = {first_mat};
                    "
                );
                for (substring, mat, dist) in rest {
                    string.push_str(&format!(
                        "
                        {substring}
                        if ({dist} < running_lowest_distance) {{
                            running_lowest_distance = {dist};
                            running_lowest_mat = {mat};
                        }}
                        "
                    ))
                }
                string.push_str(
                    "
                return vec2(running_lowest_distance, float(running_lowest_mat));
                ",
                );
                string
            }
        };

        let final_function = format!(
            "vec2 sdf(vec3 position) {{
                {object_sdfs}
            }}
            "
        );

        (final_function, materials)
    }
}

impl Material {
    fn create_shader_form(&self) -> String {
        let rotation = cgmath::Quaternion::from_arc(self.rotation.from, self.rotation.to, None);
        format!("Material(vec4({}, {}, {}, {}), vec4({}, {}, {}, {}), vec4({}, {}, {}, {}), vec4({}, {}, {}, {}), vec4({}, {}, {}, {}))", 
        self.color.x, self.color.y, self.color.z, self.translation.x, self.emitance.x, self.emitance.y, self.emitance.z, self.translation.y, self.metalness, self.roughness, self.is_portal as u8 as f32, self.translation.z, self.rotate_around.x, self.rotate_around.y, self.rotate_around.z, 0.0, rotation.v.x, rotation.v.y, rotation.v.z, rotation.s
        )
    }
}

impl Object {
    fn create_shader_form(
        &self,
        identifier_generator: &mut IdentGen,
        material_to_index: &HashMap<String, usize>,
    ) -> (String, String, String) {
        match self {
            Object::Sphere {
                center,
                radius,
                material,
            } => {
                let material_return = identifier_generator.gen("material");
                let dist_return = identifier_generator.gen("distance");
                let string = format!(
                    "
                    uint {material_return} = {};
                    float {dist_return} = distance(vec3({},{},{}), position) - {radius};
                    ",
                    material_to_index[material], center.x, center.y, center.z
                );
                return (string, material_return, dist_return);
            }
            Object::Box {
                lower_corner,
                upper_corner,
                material,
            } => {
                let material_return = identifier_generator.gen("material");
                let dist_return = identifier_generator.gen("distance");
                let lower_corner_name = identifier_generator.gen("lower_corner");
                let upper_corner_name = identifier_generator.gen("upper_corner");
                let center = identifier_generator.gen("center");
                let b = identifier_generator.gen("b");
                let q = identifier_generator.gen("q");
                let dist = identifier_generator.gen("dist");
                let string = format!(
                    "
                    uint {material_return} = {};
                    vec3 {lower_corner_name} = vec3({},{},{});
                    vec3 {upper_corner_name} = vec3({},{},{});
                    vec3 {center} = ({lower_corner_name} + {upper_corner_name}) / 2.0;
                    vec3 {b} = {center} - {lower_corner_name};
                    vec3 {q} = abs({center} - position) - {b};
                    float {dist} = distance(max({q}, vec3(0.0,0.0,0.0)), vec3(0.0,0.0,0.0));
                    float {dist_return} = {dist} + min(max(max({q}.x, {q}.y), {q}.z), 0.0);
                    ",
                    material_to_index[material],
                    lower_corner.x,
                    lower_corner.y,
                    lower_corner.z,
                    upper_corner.x,
                    upper_corner.y,
                    upper_corner.z
                );
                return (string, material_return, dist_return);
            }
            Object::Cylinder { center, height, radius, material } => {
                let material_return = identifier_generator.gen("material");
                let dist_return = identifier_generator.gen("distance");
                let cyl_center = identifier_generator.gen("center");

                let d = identifier_generator.gen("d");

                let string = format!(
                    "
                    uint {material_return} = {};
                    vec3 {cyl_center} = vec3({},{},{}) - position;

                    vec2 {d} = abs(vec2(length({cyl_center}.xz),{cyl_center}.y)) - vec2({},{});
                    float {dist_return} = min(max({d}.x,{d}.y),0.0) + length(max({d},0.0));
                    ", material_to_index[material], center.x, center.y, center.z, radius, height
                );
                return (string, material_return, dist_return)
            }
            Object::PosModulo(_, _) => todo!(),
            Object::Inv(subobject) => {
                let (substring, mat_return, sub_dist_return) =
                    subobject.create_shader_form(identifier_generator, material_to_index);
                let dist_return = identifier_generator.gen("distance");
                let string = format!(
                    "{substring}
                    float {dist_return} = -{sub_dist_return};
                    "
                );
                return (string, mat_return, dist_return);
            }
            Object::Min(obj1, obj2) => {
                let material_return = identifier_generator.gen("material");
                let dist_return = identifier_generator.gen("distance");
                let (substring1, mat1, dist1) =
                    obj1.create_shader_form(identifier_generator, material_to_index);
                let (substring2, mat2, dist2) =
                    obj2.create_shader_form(identifier_generator, material_to_index);
                let string = format!(
                    "{substring1}\n{substring2}
                    float {dist_return};
                    uint {material_return};
                    if({dist1} < {dist2}) {{
                        {dist_return} = {dist1};
                        {material_return} = {mat1};
                    }} else {{
                        {dist_return} = {dist2};
                        {material_return} = {mat2};
                    }}
                    "
                );
                return (string, material_return, dist_return);
            }
            Object::Max(obj1, obj2) => {
                let material_return = identifier_generator.gen("material");
                let dist_return = identifier_generator.gen("distance");
                let (substring1, mat1, dist1) =
                    obj1.create_shader_form(identifier_generator, material_to_index);
                let (substring2, mat2, dist2) =
                    obj2.create_shader_form(identifier_generator, material_to_index);
                let string = format!(
                    "{substring1}\n{substring2}
                    float {dist_return};
                    uint {material_return};
                    if({dist1} > {dist2}) {{
                        {dist_return} = {dist1};
                        {material_return} = {mat1};
                    }} else {{
                        {dist_return} = {dist2};
                        {material_return} = {mat2};
                    }}
                    "
                );
                return (string, material_return, dist_return);
            }
            Object::Torus {
                major_radius,
                minor_radius,
                center,
                material,
            } => {
                let material_return = identifier_generator.gen("material");
                let dist_return = identifier_generator.gen("distance");
                let point = identifier_generator.gen("point");
                let move_by = identifier_generator.gen("move_by");
                let string = format!(
                    "
                    uint {material_return} = {};
                    vec3 {point} = vec3({},{},{}) - position;
                    vec3 {move_by} = {point};
                    {move_by}.y = 0;
                    if ({move_by} == vec3(0.0,0.0,0.0)) {{
                        {move_by} = vec3(1.0,0.0,0.0);
                    }}
                    {move_by} = {} * normalize({move_by});
                    {point} -= {move_by};

                    float {dist_return} = length({point}) - {};
                    ",
                    material_to_index[material],
                    center.x,
                    center.y,
                    center.z,
                    major_radius,
                    minor_radius
                );

                return (string, material_return, dist_return);
            }
        }
    }
}
