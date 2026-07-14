use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use bevy::prelude::*;
use bevy_asset::RenderAssetUsages;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use v4l::control::Flags;


#[derive(Debug, Default)]
struct LastFrame {
        data: Vec<u8>,
        readed: bool,
}

#[derive(Resource, Default)]
struct CameraImageRaw(Arc<Mutex<LastFrame>>);

#[derive(Resource, Default)]
struct CurrentDevice(Option<(usize, v4l::Device)>);

#[derive(Resource, Default)]
struct DeviceThread(Option<V4lOut>);

type V4lOut = (
        Arc<Mutex<bool>>,
        JoinHandle<Result<(), Box<dyn std::error::Error + Send>>>,
);

fn main() {
        //let (camera_raw, _v4l_thread) = v4l_setup();

        App::new()
                .add_message::<CameraChanged>()
                .init_resource::<Messages<CameraChanged>>()
                .add_plugins(DefaultPlugins)
                .add_plugins(EguiPlugin::default())
                .insert_resource(CurrentDevice::default())
                .insert_resource(DeviceThread::default())
                .insert_resource(CameraImageRaw::default())
                .add_systems(Startup, setup)
                .add_systems(EguiPrimaryContextPass, choose_device)
                .add_systems(EguiPrimaryContextPass, config_camera)
                .add_systems(Update, read_image)
                .add_systems(Update, open_camera)
                .run();
}

#[derive(Message)]
struct CameraChanged(usize);

fn open_camera(
        mut opt_current_device: ResMut<CurrentDevice>,
        mut opt_device_thread: ResMut<DeviceThread>,
        camera_image_raw: ResMut<CameraImageRaw>,
        mut message: MessageReader<CameraChanged>) 
{

        use v4l::buffer::Type;
        use v4l::io::traits::CaptureStream;
        use v4l::prelude::*;

        if message.is_empty() {
                return;
        }

        if let Some(device_thread) = opt_device_thread.0.as_mut() {
                *device_thread.0.lock().unwrap() = true;
                while !device_thread.1.is_finished() {}
        }

        let mut f_error = |error: Box<dyn std::error::Error> | {
                warn!("{error}");
                *opt_current_device = Default::default();
                *opt_device_thread = Default::default();
        };

        let id= message.read().last().unwrap().0;

        let dev = match Device::new(id) {
                Ok(dev) => dev,
                Err(error) => {
                        f_error(Box::new(error));
                        return;
                }
        };

        let mut stream = match MmapStream::with_buffers(&dev, Type::VideoCapture, 4) {
                Ok(stream) => stream,
                Err(error) => {
                        let caps = dev.query_caps().expect("query_caps failed");
                        println!("{:#?}", caps);
                        warn!("Unspported buffer type\n{error}");
                        f_error(Box::new(error));
                        return;
                }
        };

        let camera_image2 = camera_image_raw.0.clone();
        let stop_bit = Arc::new(Mutex::new(false));
        let clone_stop_bit = stop_bit.clone();
        let spawn = move || {
                let camera_image = camera_image2;
                let stop_bit = clone_stop_bit;
                loop {
                        let (buf, _meta) = stream.next().map_err(|err| {
                                Box::new(err) as Box<dyn std::error::Error + Send>
                        })?;
                        let mut image = camera_image.lock().unwrap();
                        image.data.clear();
                        image.data.extend_from_slice(buf);
                        image.readed = false;
                        if *stop_bit.lock().unwrap() {
                                break;
                        }
                }
                Ok(())
        };

        *opt_current_device = CurrentDevice(Some((id, dev)));
        *opt_device_thread = DeviceThread(Some((stop_bit, std::thread::spawn(spawn))));
}

fn config_camera(mut contexts: EguiContexts, mut opt_current_device: ResMut<CurrentDevice>) -> Result {
        use v4l::control::Value;
        egui::Window::new("Camera Config").show(contexts.ctx_mut()?, |ui| {
                if let Some((id, device)) = opt_current_device.0.as_mut() {
                        if ui.button("Reset").clicked() {
                                for control_desc in device.query_controls().unwrap() {
                                        let mut control = match device.control(control_desc.id) {
                                                Ok(x) => x,
                                                Err(_) => continue,
                                        };
                                        control.value = match control.value {
                                            v4l::control::Value::None => Value::None,
                                            v4l::control::Value::Integer(_) => Value::Integer(control_desc.default),
                                            v4l::control::Value::Boolean(_) => Value::Boolean(control_desc.default != 0),
                                            v4l::control::Value::String(_) => continue,
                                            v4l::control::Value::CompoundU8(items) => continue,
                                            v4l::control::Value::CompoundU16(items) => continue,
                                            v4l::control::Value::CompoundU32(items) => continue,
                                            v4l::control::Value::CompoundPtr(items) => continue,
                                        };
                                        device.set_control(control);
                                }
                        }
                        for control_desc in device.query_controls().unwrap() {
                                let mut control = match device.control(control_desc.id) {
                                        Ok(control) => control,
                                        Err(_error) => {
                                                continue;
                                        },
                                };
                                ui.horizontal(|ui| {
                                        match control.value {
                                            v4l::control::Value::None => {},
                                        v4l::control::Value::Integer(mut value) => {
                                                ui.label(control_desc.name);
                                                let enabled = if control_desc.flags.contains(Flags::DISABLED) |
                                                        control_desc.flags.contains(Flags::INACTIVE) {
                                                                false
                                                        } else {
                                                                true
                                                };
                                                let slider = ui.add_enabled(
                                                        enabled,
                                                        egui::Slider::new(&mut value, control_desc.minimum..=control_desc.maximum)
                                                        .step_by(control_desc.step as f64));
                                                if slider.changed() {
                                                        control.value = v4l::control::Value::Integer(value);
                                                        match device.set_control(control) {
                                                                Ok(()) => {},
                                                                Err(error) => {
                                                                        warn!("{error}");
                                                                }
                                                        }
                                                }
                                            },
                                            v4l::control::Value::Boolean(mut value) => {
                                                let enabled = if control_desc.flags.contains(Flags::DISABLED) |
                                                        control_desc.flags.contains(Flags::INACTIVE) {
                                                                false
                                                        } else {
                                                                true
                                                        };
                                                let item = ui.add_enabled(
                                                        enabled,
                                                        egui::Checkbox::new(&mut value, control_desc.name)
                                                );
                                                if item.changed() {
                                                        control.value = v4l::control::Value::Boolean(value);
                                                        match device.set_control(control) {
                                                                Ok(()) => {},
                                                                Err(error) => {
                                                                        warn!("{error}");
                                                                }
                                                        }
                                                }
                                            },
                                            v4l::control::Value::String(_) => {},
                                            v4l::control::Value::CompoundU8(items) => {},
                                            v4l::control::Value::CompoundU16(items) => {},
                                            v4l::control::Value::CompoundU32(items) => {},
                                            v4l::control::Value::CompoundPtr(items) => {},
                                        }
                                });
                        }
                }
        });
        Ok(())
}

fn choose_device(mut contexts: EguiContexts, mut opt_current_device: ResMut<CurrentDevice>, mut messages: MessageWriter<CameraChanged>) -> Result {
        egui::Window::new("Camera Selector").show(contexts.ctx_mut()?, |ui| {
                ui.label("Connected Devices:");
                
                let devices = v4l::context::enum_devices();

                let current_id = opt_current_device.0.as_ref().map(|(id, _)| *id);

                let mut set_camera = |id: usize| {
                        if let Ok(device) = v4l::Device::new(id) {
                                opt_current_device.0 = Some((id, device));
                                messages.write(CameraChanged(id));
                        } else {
                                warn!("Failed to open camera: {id}");
                        }
                };

                for dev in devices {
                        let label = ui.label(dev.name().unwrap());
                        let id = dev.index();

                        if let Some(idx) = &current_id {
                                if id == *idx {
                                        label.highlight();
                                } else if label.clicked() {
                                        set_camera(id);
                                }
                        } else if label.clicked() {
                                set_camera(id);
                        }
                }
        });
        Ok(())
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
