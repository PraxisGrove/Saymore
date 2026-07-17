use std::{error::Error, fs::File, io::BufWriter, path::PathBuf};

use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
use image::{DynamicImage, GenericImageView, imageops::FilterType};

const ICON_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 128];
const LARGE_ICON_SIZE: u32 = 256;

pub(super) fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let (master_path, output_path) = paths_from_args(args)?;
    let master = image::ImageReader::open(&master_path)?.decode()?;
    validate_master(&master)?;

    let file = File::create(&output_path)?;
    let mut directory = IconDir::new(ResourceType::Icon);
    for size in ICON_SIZES.into_iter().chain([LARGE_ICON_SIZE]) {
        let rgba = master
            .resize_exact(size, size, FilterType::Lanczos3)
            .to_rgba8();
        let image = IconImage::from_rgba_data(size, size, rgba.into_raw());
        directory.add_entry(IconDirEntry::encode(&image)?);
    }
    directory.write(BufWriter::new(file))?;
    verify_output(&output_path)?;
    println!(
        "[INFO] generated {} from {} with sizes 16,20,24,32,40,48,64,128,256",
        output_path.display(),
        master_path.display()
    );
    Ok(())
}

fn paths_from_args(args: &[String]) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let mut master = PathBuf::from("apps/desktop/icons/icon-master.png");
    let mut output = PathBuf::from("apps/desktop/icons/icon.ico");
    let mut index = 0;
    while index < args.len() {
        let target = match args[index].as_str() {
            "--master" => &mut master,
            "--output" => &mut output,
            other => return Err(format!("unknown windows-icons option: {other}").into()),
        };
        *target = PathBuf::from(
            args.get(index + 1)
                .ok_or_else(|| format!("missing value for {}", args[index]))?,
        );
        index += 2;
    }
    Ok((master, output))
}

fn validate_master(master: &DynamicImage) -> Result<(), Box<dyn Error>> {
    if master.dimensions() != (1024, 1024) {
        return Err("Windows icon master must be 1024x1024".into());
    }
    Ok(())
}

fn verify_output(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let directory = IconDir::read(File::open(path)?)?;
    let actual: Vec<u32> = directory
        .entries()
        .iter()
        .map(IconDirEntry::width)
        .collect();
    let expected: Vec<u32> = ICON_SIZES.into_iter().chain([LARGE_ICON_SIZE]).collect();
    if actual != expected {
        return Err(format!("ICO dimensions are incomplete: {actual:?}").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_non_square_master() {
        let master = DynamicImage::new_rgba8(1024, 512);
        assert!(validate_master(&master).is_err());
    }

    #[test]
    fn accepts_a_square_master() {
        let master = DynamicImage::new_rgba8(1024, 1024);
        assert!(validate_master(&master).is_ok());
    }
}
