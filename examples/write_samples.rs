use cadrum::{DVec3, Solid};
use std::fs::File;
use std::path::Path;

fn main() -> Result<(), cadrum::Error> {
    let dir = Path::new("samples/generated");
    std::fs::create_dir_all(dir).expect("create samples/generated");

    let cube = Solid::cube(DVec3::ZERO, DVec3::new(20.0, 15.0, 10.0)).color("#4a90d9");
    let cyl = Solid::cylinder(4.0, DVec3::Z * 18.0)
        .translate(DVec3::X * 30.0)
        .color("#e67e22");
    let sph = Solid::sphere(6.0)
        .translate(DVec3::X * 55.0 + DVec3::Z * 6.0)
        .color("#2ecc71");
    let solids = [cube, cyl, sph];

    Solid::write_step(
        &solids,
        &mut File::create(dir.join("primitives.step")).unwrap(),
    )?;
    Solid::write_brep(
        &solids,
        &mut File::create(dir.join("primitives.brep")).unwrap(),
    )?;
    let mesh = Solid::mesh(&solids, Default::default())?;
    mesh.write_stl(&mut File::create(dir.join("primitives.stl")).unwrap())?;

    let block = Solid::cube(DVec3::ZERO, DVec3::splat(20.0));
    let hole = Solid::cylinder(5.0, DVec3::Z * 24.0).translate(DVec3::new(10.0, 10.0, -2.0));
    let cut = (&block - &hole).build_vec()?;
    Solid::write_step(
        &cut,
        &mut File::create(dir.join("block_with_hole.step")).unwrap(),
    )?;
    Ok(())
}
