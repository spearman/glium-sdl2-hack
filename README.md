# `glium_sdl2_hack`

> An [**unsafe**](#unsafety) backend for SDL + Glium that makes it possible to
> collect input events on the main thread while rendering on a child thread.

[Documentation](https://spearman.github.io/glium-sdl2-hack/glium_sdl2_hack/index.html)

Relies on modified `glium` and `sdl2` libraries to achieve the following
low-level order of operations:

```c
  // on main thread
  // initializing video also initializes event subsystem
  SDL_InitSubsystem (SDL_INIT_VIDEO);
  // set gl attributes here (not shown) before window creation
  SDL_Window* window = SDL_CreateWindow (
    "my window",
    SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED, 320, 240,
    SDL_WINDOW_OPENGL);
  SDL_GLContext context = SDL_GL_CreateContext (window);
  // load opengl
  gladLoadGL();
  SDL_GL_MakeCurrent (NULL, NULL); // context must be released on the main thread

  // spawn a child thread which can access the window and context variables
  { // on child (render) thread
    SDL_GL_MakeCurrent (window, context);
    // opengl can now be called
    glClearColor (0.0, 0.0, 1.0, 1.0);
    glClear (GL_COLOR_BUFFER_BIT);
    SDL_GL_SwapWindow (window);
  }

  // back on the main thread we can poll or wait on events
  { // on main (input) thread
    SDL_Event event;
    while (SDL_WaitEvent (&event)) {
      // handle events
    }
  }
```

## Usage

Requires custom `glium` and `sdl2` dependencies:

```toml
[dependencies.sdl2]
version = "0.31.*"
git = "git://github.com/spearman/rust-sdl2.git"
branch = "hack"

[dependencies.glium]
version = "0.19.*"
git = "git://github.com/spearman/glium.git"
branch = "hack"
features = []
default-features = false

[dependencies.glium-sdl2-hack]
version = "0.1.*"
git = "git://github.com/spearman/glium-sdl2-hack.git"
```

An example program:

```rust
//! Spawn a small window that cycles between green and red every 50 frames. 'Q'
//! or 'Escape' to quit.

extern crate sdl2;
extern crate glium;
extern crate glium_sdl2_hack;

static RUNNING : std::sync::atomic::AtomicBool
  = std::sync::atomic::ATOMIC_BOOL_INIT;

fn main () {
  RUNNING.store (true, std::sync::atomic::Ordering::SeqCst);

  // init sdl window
  let sdl_context     = sdl2::init().unwrap();
  let video_subsystem = sdl_context.video().unwrap();
  use glium_sdl2_hack::SdlGlWindowBuilder;
  let window_backend  = video_subsystem.window ("my window", 320, 240)
    .position_centered()
    .build_backend().unwrap();

  let input_thread  = std::thread::current();

  // render thread
  let render_handle = std::thread::spawn (move || {
    // acquire the glium display facade
    let mut display_facade = window_backend.build_glium().unwrap();

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
  });

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
    }
  }

  render_handle.join().unwrap();
}
```

## Unsafety

Because `sdl2::VideoSubsystem` is not transferrable accross threads, there is
no gurantee that the video subsystem will not be quit on the main thread while
the window and render context is still alive on the child. Therefore, *it must
be ensured that the original video subsystem that spawned the window does not
drop before the acquired window.*

It is also possible to acquire a "fake" reference to the `sdl2::VideoSubsystem`
indirectly through the `Display::window` method, but it is not possible to
create a window on a thread other than main, so the
`sdl2::VideoSubsystem::window` method to build a new window **should not be
called from a child thread**.

## FAQ

Q. *Why?*

A. Decoupling. Having an input thread separate from rendering means your
simulation or network threads can still receive input even if rendering is
blocked waiting for vsync or the framerate drops. Theoretically this should
reduce input latency by an average of 8ms if inputs are assumed to be normally
distributed and rendering is spending most of its time waiting for vsync. Does
this mean that inputs are *displayed* with less latency? No-- it may even be
the case that the SDL event pump will not receive events while the window is
blocked on vsync. However, it does still enable the threads to be decoupled at
least programmatically.

Q. *Is it safe?*

A. No!

Q. *Should I use it?*

A. If you are asking then the answer is no!

Q. *What kind of games is this intended for?*

A. This is mainly intended for networked games with fast player movement (think
Doom and Quake) where the render thread is already separate from the simulation
thread. If your game is turn-based or doesn't rely on input timing it will
probably not be useful. Note that even for single-player games, if input timing
is important then this decoupling is desireable. Players don't wait for a frame
to be rendered before deciding what input to press. Rather, input is mentally
"scheduled" ahead of time, so in the case that the renderer decides to hang, as
long as the simulation thread can continue to receive input, your timed "jump"
command won't be missed or delayed due to the hang.

Q. *Does it work?*

A. So far it is only tested on a total of 2 machines, one running Linux Mint
17.3 and the other Windows 7, both with NVIDIA drivers. On Windows 7, when
tested with an MSYS2 setup, the mintty terminal does not show any stdout print
statements until the window is closed, but otherwise the window is responsive.

Q. *Can it work?*

A. Supposedly id Tech 3/4 engines use OpenGL for rendering and collect input
["in a background thread"](https://www.gamedev.net/forums/topic/656813-want-to-get-input-messages-instantly-while-waiting-for-vsync-blocked-swapbuffers/?tab=comments#comment-5154701).
I haven't confirmed this myself. Whether or not there are any existing SDL
applications using this method is unknown.

## Acknowledgements

The overall implementation follows very closely the *safe* SDL2 + Glium backend
found in the [`glium-sdl2`](https://github.com/nukep/glium-sdl2/) crate.
