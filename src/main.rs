use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use bevy::prelude::*;
use bevy_asset::RenderAssetUsages;
use v4l::context;

#[derive(Debug, Default)]
struct LastFrame {
        data: Vec<u8>,
        readed: bool,
}

#[derive(Resource, Default)]
struct CameraImageRaw(Arc<Mutex<LastFrame>>);

type V4lOut = (
        CameraImageRaw,
        JoinHandle<Result<(), Box<dyn std::error::Error + Send>>>,
);

fn v4l_setup() -> V4lOut {
        use v4l::buffer::Type;
        use v4l::io::traits::CaptureStream;
        use v4l::prelude::*;

        let dev = Device::new(0).unwrap_or_else(|error| {
                let devices = context::enum_devices();

                for dev in devices {
                        println!("Index: {}", dev.index());
                        println!("Name: {}", dev.name().unwrap());
                }
                panic!("{error}")
        });

        let camera_image = CameraImageRaw::default();

        let mut stream = MmapStream::with_buffers(&dev, Type::VideoCapture, 4)
                .unwrap_or_else(|error| {
                        let caps = dev.query_caps().expect("query_caps failed");
                        println!("{:#?}", caps);
                        panic!("Unspported buffer type\n{error}");
                });

        let camera_image2 = camera_image.0.clone();
        let spawn = move || {
                let camera_image = camera_image2;
                loop {
                        let (buf, _meta) = stream.next().map_err(|err| {
                                Box::new(err) as Box<dyn std::error::Error + Send>
                        })?;
                        let mut image = camera_image.lock().unwrap();
                        image.data.clear();
                        image.data.extend_from_slice(buf);
                        image.readed = false;
                }
        };

        (camera_image, std::thread::spawn(spawn))
}

fn main() {
        let (camera_raw, _v4l_thread) = v4l_setup();

        App::new()
                .add_plugins(DefaultPlugins)
                .insert_resource(camera_raw)
                .add_systems(Startup, setup)
                .add_systems(Update, read_image)
                .run();
}

fn setup(mut commands: Commands) {
        commands.spawn(Camera2d);

        commands.spawn((Sprite::default(), Camera));
}

fn read_image(
        sprite: Single<&mut Sprite, With<Camera>>,
        raw: Res<CameraImageRaw>,
        mut images: ResMut<Assets<Image>>,
) {
        let image_raw = {
                let mut raw = raw.0.lock().unwrap();
                if raw.readed {
                        return;
                }
                raw.readed = true;
                raw.data.clone()
        };

        let image_dyn = match image::load_from_memory(&image_raw) {
                Ok(image_dyn) => image_dyn,
                Err(err) => {
                        info!("Error decoding input image: \n {err}");
                        return;
                }
        };
        let image_bevy = Image::from_dynamic(image_dyn, false, RenderAssetUsages::all());
        let image_handle = images.add(image_bevy);
        *sprite.into_inner() = Sprite::from_image(image_handle.clone());
}

#[derive(Component)]
struct Camera;
