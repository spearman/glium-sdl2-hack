//! Spawn a small window that cycles between green and red every 50 frames. 'Q'
//! or 'Escape' to quit.

extern crate sdl2;
extern crate glium;
extern crate glium_sdl2_hack;

static RUNNING : std::sync::atomic::AtomicBool
  = std::sync::atomic::ATOMIC_BOOL_INIT;

fn main () {
  println!("example main...");
  RUNNING.store (true, std::sync::atomic::Ordering::SeqCst);

  println!("size of SdlGlWindowBackend: {}",
    std::mem::size_of::<glium_sdl2_hack::SdlGlWindowBackend>());

  // init
  let sdl_context     = sdl2::init().unwrap();
  let video_subsystem = sdl_context.video().unwrap();
  use glium_sdl2_hack::SdlGlWindowBuilder;
  let window_backend  = video_subsystem.window ("my window", 320, 240)
    .position_centered()
    .build_backend().unwrap();

  let input_thread  = std::thread::current();

  // render thread
  let render_handle = std::thread::spawn (move || {

    // acquire the display facade
    let mut display_facade = window_backend.build_glium().unwrap();
    { // test that we can operate on the window
      let window = unsafe { display_facade.window_mut() };
      println!("title: {}", window.title());
      window.set_title ("new title").unwrap();
      println!("title: {}", window.title());
    }

    input_thread.unpark();

    let mut frame = 0;
    'frameloop: while RUNNING.load (std::sync::atomic::Ordering::SeqCst) {
      use glium::Surface;

      if frame % 60 == 0 {
        println!("frame: {}", frame);
      }

      let clear_color = if 50 < frame % 100 {
        (1.0, 0.0, 0.0, 1.0)
      } else {
        (0.0, 1.0, 0.0, 1.0)
      };

      let mut glium_frame = display_facade.draw();
      glium_frame.clear_all (clear_color, 0.0, 0);
      glium_frame.finish().unwrap();

      frame += 1;
    }
  }); // end render thread

  std::thread::park();  // wait for render thread to start

  // sdl input events
  let mut event_pump = sdl_context.event_pump().unwrap();
  'inputloop: loop {
    let event = event_pump.wait_event();
    println!("{:?}", event);
    match event {
      sdl2::event::Event::KeyDown {
        keycode: Some(keycode), ..
      } => {
        match keycode {
          sdl2::keyboard::Keycode::Q | sdl2::keyboard::Keycode::Escape => {
            RUNNING.store (false, std::sync::atomic::Ordering::SeqCst);
            break 'inputloop;
          },
          _ => {}
        }
      }
      _ => ()
    } // end match event
  } // end sdl input events

  render_handle.join().unwrap();
  println!("...example main");
}
