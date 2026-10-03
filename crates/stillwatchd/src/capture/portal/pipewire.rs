//! `PipeWire` consumer for one portal remote.
//!
//! The loop runs on its own thread. Each stream is asked for one frame per
//! second. Extra buffers are dequeued and dropped without being read.
//! `MAP_BUFFERS` asks `PipeWire` to map memfd and `DMA-BUF` memory; a buffer
//! it didn't map is an error, because Stillwatch doesn't map file descriptors
//! itself. The mapped bytes are copied, downscaled, and dropped before the
//! buffer goes back to the pool.

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pipewire::context::ContextBox;
use pipewire::core::Core;
use pipewire::main_loop::MainLoopRc;
use pipewire::properties::properties;
use pipewire::spa::param::video::VideoInfoRaw;
use pipewire::spa::pod::serialize::PodSerializer;
use pipewire::spa::utils::{Direction, Fraction, Rectangle};
use pipewire::stream::{StreamBox, StreamFlags};
use pipewire::{keys, spa};
use stillwatch_core::backend::BackendError;

use super::cache::FrameCache;
use super::error::map_pipewire;
use super::frame::{self, Plane};

/// How long a buffer is ignored after one was kept, so a compositor that
/// ignores the 1 fps request doesn't get downscaled at its full rate.
const FRAME_GAP: Duration = Duration::from_millis(900);

/// One portal stream that was matched to a connector.
pub(crate) struct Target {
    pub(crate) node_id: u32,
    pub(crate) output: String,
}

/// The thread running the `PipeWire` loop.
pub(crate) struct PipeThread {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl PipeThread {
    /// Connects `remote` and consumes `targets` until [`stop`](Self::stop).
    ///
    /// # Errors
    ///
    /// [`BackendError::Disconnected`] when `PipeWire` can't be created or the
    /// remote can't be connected.
    pub(crate) async fn spawn(
        remote: OwnedFd,
        targets: Vec<Target>,
        cache: Arc<FrameCache>,
    ) -> Result<Self, BackendError> {
        let (tx, rx) = sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let join = std::thread::spawn(move || {
            if let Err(error) = drive(remote, targets, &cache, &stop_thread, &tx) {
                let _ = tx.send(Err(error));
            }
        });
        let started = tokio::task::spawn_blocking(move || rx.recv())
            .await
            .map_err(|err| BackendError::Disconnected(format!("PipeWire thread: {err}")))?
            .map_err(|_| {
                BackendError::Disconnected("PipeWire thread exited before it was ready".into())
            })?;
        started?;
        Ok(Self {
            stop,
            join: Some(join),
        })
    }

    /// Asks the loop to quit and waits for the thread.
    pub(crate) fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            tracing::debug!("PipeWire capture thread panicked");
        }
    }
}

impl Drop for PipeThread {
    fn drop(&mut self) {
        self.stop();
    }
}

struct NodeData {
    output: String,
    cache: Arc<FrameCache>,
    format: Option<FormatInfo>,
    last: Option<Instant>,
}

struct FormatInfo {
    code: u32,
    width: u32,
    height: u32,
}

fn drive(
    remote: OwnedFd,
    targets: Vec<Target>,
    cache: &Arc<FrameCache>,
    stop: &Arc<AtomicBool>,
    ready: &SyncSender<Result<(), BackendError>>,
) -> Result<(), BackendError> {
    let mainloop = MainLoopRc::new(None).map_err(|error| map_pipewire(&error))?;
    let runner = mainloop.clone();
    let timer = mainloop.loop_().add_timer({
        let mainloop = mainloop.clone();
        let stop = Arc::clone(stop);
        move |_| {
            if stop.load(Ordering::Relaxed) {
                mainloop.quit();
            }
        }
    });
    timer.update_timer(
        Some(Duration::from_millis(50)),
        Some(Duration::from_millis(200)),
    );

    let context = ContextBox::new(mainloop.loop_(), None).map_err(|error| map_pipewire(&error))?;
    let core = context
        .connect_fd(remote, None)
        .map_err(|error| map_pipewire(&error))?;
    let mut streams = Vec::with_capacity(targets.len());
    let mut listeners = Vec::with_capacity(targets.len());
    let mut formats = Vec::with_capacity(targets.len());
    for target in targets {
        let (stream, listener, bytes) = connect_node(&core, target, Arc::clone(cache))?;
        formats.push(bytes);
        listeners.push(listener);
        streams.push(stream);
    }
    ready
        .send(Ok(()))
        .map_err(|_| BackendError::Disconnected("PipeWire ready channel closed".into()))?;
    runner.run();
    drop(streams);
    drop(listeners);
    drop(formats);
    drop(timer);
    Ok(())
}

fn connect_node(
    core: &Core,
    target: Target,
    cache: Arc<FrameCache>,
) -> Result<
    (
        StreamBox<'_>,
        pipewire::stream::StreamListener<NodeData>,
        Vec<u8>,
    ),
    BackendError,
> {
    let stream = StreamBox::new(
        core,
        "stillwatch",
        properties! {
            *keys::MEDIA_TYPE => "Video",
            *keys::MEDIA_CATEGORY => "Capture",
            *keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|error| map_pipewire(&error))?;
    let listener = stream
        .add_local_listener_with_user_data(NodeData {
            output: target.output,
            cache,
            format: None,
            last: None,
        })
        .state_changed(|_, data, _, new| {
            if let pipewire::stream::StreamState::Error(error) = new {
                tracing::debug!(output = %data.output, %error, "portal stream error");
                data.cache
                    .fail(BackendError::Disconnected(format!("PipeWire: {error}")));
            }
        })
        .param_changed(|_, data, id, param| remember_format(data, id, param))
        .process(keep_one_frame)
        .register()
        .map_err(|error| map_pipewire(&error))?;
    let bytes = format_bytes()?;
    let pod = spa::pod::Pod::from_bytes(&bytes)
        .ok_or_else(|| BackendError::Protocol("PipeWire format pod is empty".into()))?;
    let mut params = [pod];
    stream
        .connect(
            Direction::Input,
            Some(target.node_id),
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|error| map_pipewire(&error))?;
    Ok((stream, listener, bytes))
}

fn remember_format(data: &mut NodeData, id: u32, param: Option<&spa::pod::Pod>) {
    let Some(param) = param else {
        return;
    };
    if id != spa::param::ParamType::Format.as_raw() {
        return;
    }
    let Ok((media, subtype)) = spa::param::format_utils::parse_format(param) else {
        return;
    };
    if media != spa::param::format::MediaType::Video
        || subtype != spa::param::format::MediaSubtype::Raw
    {
        return;
    }
    let mut info = VideoInfoRaw::default();
    if info.parse(param).is_err() {
        return;
    }
    let size = info.size();
    data.format = Some(FormatInfo {
        code: spa_code(info.format()),
        width: size.width,
        height: size.height,
    });
    tracing::debug!(
        output = %data.output,
        format = data.format.as_ref().map(|format| format.code),
        width = size.width,
        height = size.height,
        "portal stream format"
    );
}

fn keep_one_frame(stream: &pipewire::stream::Stream, data: &mut NodeData) {
    let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
    };
    let now = Instant::now();
    if data
        .last
        .is_some_and(|last| now.saturating_duration_since(last) < FRAME_GAP)
    {
        return;
    }
    let Some(format) = data.format.as_ref() else {
        return;
    };
    let spa_format = format.code;
    let width = format.width;
    let height = format.height;
    let copied = {
        let datas = buffer.datas_mut();
        let Some(plane) = datas.first_mut() else {
            return;
        };
        let stride = plane.chunk().stride();
        if stride < 0 {
            data.cache.fail(BackendError::Unsupported(
                "PipeWire sent a bottom-up frame".into(),
            ));
            return;
        }
        let kind = plane.type_();
        let offset = plane.chunk().offset();
        let len = plane.chunk().size();
        let stride = usize::try_from(stride).unwrap_or(0);
        let bytes = plane.data();
        (
            frame::copy_mapped(kind, bytes.as_deref(), offset, len),
            stride,
        )
    };
    let (copied, stride) = copied;
    let copied = match copied {
        Ok(copied) => copied,
        Err(error) => {
            data.cache.fail(error);
            return;
        }
    };
    let grid = frame::grid_from_plane(
        &Plane {
            spa_format,
            width,
            height,
            stride,
            data: &copied,
        },
        data.cache.width(),
    );
    drop(copied);
    match grid {
        Ok(grid) => {
            data.last = Some(now);
            data.cache.store(&data.output, data.cache.width(), grid);
        }
        Err(error) => data.cache.fail(error),
    }
}

fn spa_code(format: spa::param::video::VideoFormat) -> u32 {
    format.0
}

fn format_bytes() -> Result<Vec<u8>, BackendError> {
    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::xRGB,
            spa::param::video::VideoFormat::ARGB
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            Rectangle {
                width: 320,
                height: 240
            },
            Rectangle {
                width: 1,
                height: 1
            },
            Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            Fraction { num: 1, denom: 1 },
            Fraction { num: 0, denom: 1 },
            Fraction { num: 1, denom: 1 }
        ),
    );
    let (cursor, _) = PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|err| BackendError::Protocol(format!("PipeWire format: {err}")))?;
    Ok(cursor.into_inner())
}
