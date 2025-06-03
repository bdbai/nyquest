#![allow(non_snake_case)]

use std::io::{self, Cursor};
use std::ops::ControlFlow;
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use arc_swap::ArcSwapAny;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, ClassType, DefinedClass, Message as _};
use objc2_foundation::{
    NSArray, NSError, NSInputStream, NSInteger, NSObjectProtocol, NSRunLoop, NSRunLoopMode,
    NSStreamDelegate, NSStreamEvent, NSStreamPropertyKey, NSStreamStatus, NSString, NSUInteger,
};

use crate::datatask::GenericWaker;
use crate::retained_ext::SwappableRetained;

#[cfg(target_os = "macos")]
const STREAM_BUFFER_SIZE: usize = 1024 * 64;
#[cfg(not(target_os = "macos"))]
const STREAM_BUFFER_SIZE: usize = 1024 * 16;

pub(crate) struct InputStreamIvars {
    waker: GenericWaker,
    delegate: ArcSwapAny<Option<SwappableRetained<ProtocolObject<dyn NSStreamDelegate>>>>,
    run_loop: ArcSwapAny<Option<SwappableRetained<NSRunLoop>>>,
    run_loop_mode: ArcSwapAny<Option<SwappableRetained<NSRunLoopMode>>>,
    stream_buffer: Mutex<Result<Cursor<Vec<u8>>, Retained<NSError>>>,
    is_open: AtomicBool,
    eof: AtomicBool,
}

define_class!(
    // SAFETY:
    // - The superclass NSObject does not have any subclassing requirements.
    // - `Delegate` does not implement `Drop`.
    #[unsafe(super = NSInputStream)]
    // #[thread_kind = MainThreadOnly]
    #[name = "Nyquest_InputStream"]
    #[ivars = InputStreamIvars]
    pub(crate) struct InputStream;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for InputStream {}

    impl InputStream {
        #[unsafe(method(open))]
        fn __open(&self) {
            self.callback_open();
        }

        #[unsafe(method(close))]
        fn __close(&self) {
            self.callback_close();
        }

        #[unsafe(method(setDelegate:))]
        fn setDelegate(&self, delegate: Option<&ProtocolObject<dyn NSStreamDelegate>>) {
            self.callback_setDelegate(delegate);
        }

        #[unsafe(method(propertyForKey:))]
        fn propertyForKey(
            &self,
            _key: &NSStreamPropertyKey,
        ) -> *mut AnyObject {
            // __kCFStreamPropertyHTTPTrailer
            null_mut()
        }

        #[unsafe(method(setProperty:forKey:))]
        fn setProperty_forKey(
            &self,
            _property: Option<&AnyObject>,
            _key: &NSStreamPropertyKey,
        ) -> bool {
            false
        }

        #[unsafe(method(scheduleInRunLoop:forMode:))]
        fn scheduleInRunLoop_forMode(
            &self,
            a_run_loop: &NSRunLoop,
            mode: &NSRunLoopMode,
        ) {
            self.callback_scheduleInRunLoop_forMode(a_run_loop, mode);
        }

        #[unsafe(method(removeFromRunLoop:forMode:))]
        fn removeFromRunLoop_forMode(
            &self,
            a_run_loop: &NSRunLoop,
            mode: &NSRunLoopMode,
        ) {
            self.callback_removeFromRunLoop_forMode(a_run_loop, mode);
        }

        #[unsafe(method(streamStatus))]
        fn streamStatus(&self) -> NSStreamStatus {
            self.callback_streamStatus()
        }

        #[unsafe(method(streamError))]
        fn streamError(&self) -> *mut NSError {
            self.callback_streamError()
        }

        #[unsafe(method(read:maxLength:))]
        fn read_maxLength(&self, buffer: NonNull<u8>, len: NSUInteger) -> NSInteger {
            self.callback_read_maxLength(buffer, len)
        }

        #[unsafe(method(hasBytesAvailable))]
        fn hasBytesAvailable(&self) -> bool {
            self.callback_hasBytesAvailable()
        }
    }
);

impl InputStream {
    pub(crate) fn new(waker: GenericWaker) -> Retained<Self> {
        let this = Self::alloc().set_ivars(InputStreamIvars {
            waker,
            delegate: ArcSwapAny::new(None),
            run_loop: ArcSwapAny::new(None),
            run_loop_mode: ArcSwapAny::new(None),
            stream_buffer: Mutex::new(Ok(Cursor::new(vec![0; STREAM_BUFFER_SIZE]))),
            is_open: AtomicBool::new(false),
            eof: AtomicBool::new(false),
        });

        unsafe { msg_send![super(this), init] }
    }
    pub(crate) fn update_buffer<C>(
        &self,
        cb: impl FnOnce(&mut [u8]) -> ControlFlow<C, io::Result<usize>>,
    ) -> Option<C> {
        let ivars = self.ivars();
        if ivars.eof.load(Ordering::SeqCst) {
            return None;
        }
        let mut stream_buffer = ivars.stream_buffer.lock().unwrap();
        let Ok(cursor) = &mut *stream_buffer else {
            return None;
        };

        let pos = cursor.position() as usize;
        let buffer = &mut cursor.get_mut()[pos..];
        if !buffer.is_empty() {
            let read_res = cb(buffer);
            match read_res {
                ControlFlow::Break(c) => {
                    return Some(c);
                }
                ControlFlow::Continue(Ok(0)) => {
                    ivars.eof.store(true, Ordering::SeqCst);
                    if pos == 0 {
                        self.notify_stream_state(NSStreamEvent::EndEncountered);
                        return None;
                    }
                }
                ControlFlow::Continue(Ok(read_len)) => {
                    cursor.set_position((pos + read_len) as u64);
                }
                ControlFlow::Continue(Err(e)) => {
                    let ns_err = NSError::new(
                        e.raw_os_error().unwrap_or_default() as _,
                        &NSString::from_str(&e.to_string()),
                    );
                    *stream_buffer = Err(ns_err);
                    self.notify_stream_state(NSStreamEvent::ErrorOccurred);
                    return None;
                }
            }
        }

        if cursor.position() > 0 {
            drop(stream_buffer);
            self.notify_stream_state(NSStreamEvent::HasBytesAvailable);
        }
        None
    }

    fn notify_stream_state(&self, event: NSStreamEvent) {
        let ivars = self.ivars();
        let Some(delegate) = ivars.delegate.load_full() else {
            return;
        };
        let Some(run_loop) = ivars.run_loop.load_full() else {
            return;
        };
        let Some(run_loop_mode) = ivars.run_loop_mode.load_full() else {
            return;
        };
        unsafe {
            let stream = self.as_super().retain();
            run_loop.performInModes_block(
                &NSArray::from_retained_slice(&[run_loop_mode.retain()]),
                &RcBlock::new(move || {
                    let delegate: &ProtocolObject<dyn NSStreamDelegate> = &*delegate;
                    delegate.stream_handleEvent(&stream, event);
                }),
            );
        }
    }

    fn callback_open(&self) {
        let ivars = self.ivars();
        ivars.is_open.store(true, Ordering::Relaxed);
    }
    fn callback_close(&self) {
        let ivars = self.ivars();
        let mut stream_buffer = ivars.stream_buffer.lock().unwrap();
        let Ok(stream_buffer) = stream_buffer.as_mut() else {
            return;
        };
        stream_buffer.set_position(0);
        *stream_buffer.get_mut() = vec![];
    }
    fn callback_setDelegate(&self, delegate: Option<&ProtocolObject<dyn NSStreamDelegate>>) {
        let delegate = delegate.map(|d| d.retain().into());
        self.ivars().delegate.store(delegate);
    }
    fn callback_scheduleInRunLoop_forMode(&self, a_run_loop: &NSRunLoop, mode: &NSRunLoopMode) {
        let ivars = self.ivars();
        ivars.run_loop.store(Some(a_run_loop.retain().into()));
        ivars.run_loop_mode.store(Some(mode.retain().into()));
    }
    fn callback_removeFromRunLoop_forMode(&self, _a_run_loop: &NSRunLoop, _mode: &NSRunLoopMode) {
        let ivars = self.ivars();
        ivars.run_loop.store(None);
        ivars.run_loop_mode.store(None);
    }
    fn callback_streamStatus(&self) -> NSStreamStatus {
        let ivars = self.ivars();
        let stream_buffer = ivars.stream_buffer.lock().unwrap();
        match &*stream_buffer {
            Ok(_) if ivars.eof.load(Ordering::SeqCst) => NSStreamStatus::AtEnd,
            Ok(_) if ivars.is_open.load(Ordering::Relaxed) => NSStreamStatus::Open,
            Ok(_) => NSStreamStatus::NotOpen,
            Err(_) => NSStreamStatus::Error,
        }
    }
    fn callback_streamError(&self) -> *mut NSError {
        let ivars = self.ivars();
        let stream_buffer = ivars.stream_buffer.lock().unwrap();
        match &*stream_buffer {
            Ok(_) => std::ptr::null_mut(),
            Err(error) => Retained::into_raw(error.clone()),
        }
    }
    fn callback_read_maxLength(&self, buffer: NonNull<u8>, len: NSUInteger) -> NSInteger {
        let ivars = self.ivars();
        let mut stream_buffer = ivars.stream_buffer.lock().unwrap();

        match &mut *stream_buffer {
            Ok(cursor) => {
                let read_len = (cursor.position() as usize).min(len);
                if read_len == 0 {
                    return 0;
                }
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        cursor.get_ref().as_ptr(),
                        buffer.as_ptr(),
                        read_len,
                    );
                    cursor.get_mut().drain(..read_len);
                    // Safety: the underlying buffer will always have STREAM_BUFFER_SIZE initialized bytes
                    cursor.get_mut().set_len(STREAM_BUFFER_SIZE);
                }
                cursor.set_position(cursor.position() - read_len as u64);
                ivars.waker.wake();
                read_len as NSInteger
            }
            Err(_) => -1,
        }
    }
    fn callback_hasBytesAvailable(&self) -> bool {
        let ivars = self.ivars();
        let stream_buffer = ivars.stream_buffer.lock().unwrap();
        matches!(&*stream_buffer, Ok(cursor) if cursor.position() > 0)
    }
}

// Safety: ivars are protected by ArcSwapAny Retained and Mutex
unsafe impl Send for InputStream {}
unsafe impl Sync for InputStream {}
