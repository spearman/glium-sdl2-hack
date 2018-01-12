//! An unsafe backend for SDL + Glium that makes it possible to collect input
//! events on the main thread while rendering on a child thread. See
//! `./README.md` for more details and `./example/example.rs` for a usage
//! example.

#![feature(unique)]

extern crate glium;
extern crate sdl2;
extern crate sdl2_sys;

///////////////////////////////////////////////////////////////////////////////
//  typedefs                                                                 //
///////////////////////////////////////////////////////////////////////////////

pub type Display = SdlGliumDisplayFacade;
pub type Window  = SdlGlWindowBackend;

///////////////////////////////////////////////////////////////////////////////
//  structs                                                                  //
///////////////////////////////////////////////////////////////////////////////

//
// public
//

/// Represents the combined SDL window context + Glium rendering context.
///
/// &#9888; **Warning**: while this type allows references to the
/// `sdl2::video::Window` type (through the unsafe `window` and `window_mut`
/// methods), and from this a reference to the `sdl2::VideoSubsystem` is
/// possible, it does not contain a "real" reference to this type since it is
/// not safe to send to another thread. This is enforced because it is not safe
/// to create a window on a thread other than the main thread, so the
/// `VideoSubsystem::window` function **must not be called**.
///
/// TODO: since we already fork sdl2, could we add a global atomic flag to
/// prevent ever trying to build another window after the first ?
#[derive(Clone)]
pub struct SdlGliumDisplayFacade {
  glium_context       : std::rc::Rc <glium::backend::Context>,
  window_backend      : std::rc::Rc <SdlGlWindowBackend>,
  sdl_window_impostor : std::rc::Rc <std::cell::UnsafeCell <SdlWindowImpostor>>
}

/// This type is transferrable to another thread.
///
/// When acquired the context will already be released so all you can do with
/// it is build Glium (which will automatically re-acquire the context).
pub struct SdlGlWindowBackend {
  window_raw     : std::ptr::Unique <sdl2_sys::SDL_Window>,
  /// The intended type is:
  /// ```ignore
  /// gl_context_raw : std::ptr::Unique <sdl2_sys::SDL_GLContext>
  /// ```
  /// but this gives a `std::ptr::Unique <*mut std::os::raw::c_void>`
  /// which is not what we want.
  gl_context_raw : std::ptr::Unique <std::os::raw::c_void>,
  gl_funs        : Option <Box <glium::gl::Gl>>
}

//
// private
//

/// Type used to transmute into an `sdl2::video::Window`.
///
/// It is important that only references to the transmuted value are given out
/// so that resources are not freed when dropped.
#[derive(Clone)]
struct SdlWindowImpostor {
  window_context_impostor : std::rc::Rc <SdlWindowContextImpostor>
}

/// Type transmuted into an `sdl2::video::WindowContext`.
///
/// This will not be accessible directly, but any functions on the referring
/// window that attempt to *clone* the video subsystem **should not be called**
/// as it will contain a NULL `Rc` pointer.
struct SdlWindowContextImpostor {
  /// `VideoSubsystem` is a single (unused) `Rc` drop token.
  _video_subsystem : std::rc::Rc <()>,
  _window_raw      : *mut sdl2_sys::SDL_Window
}

///////////////////////////////////////////////////////////////////////////////
//  enums                                                                    //
///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub enum BackendBuildError {
  WindowBuildError     (sdl2::video::WindowBuildError),
  ContextCreationError (String)
}

///////////////////////////////////////////////////////////////////////////////
//  traits                                                                   //
///////////////////////////////////////////////////////////////////////////////

/// Implementing this trait for `sdl2::video::WindowBuilder` makes creating a
/// new window backend a little more ergonomic.
pub trait SdlGlWindowBuilder {
  /// Builds a window backend and releases the context.
  fn build_backend (&mut self) -> Result <SdlGlWindowBackend, BackendBuildError>;
}

///////////////////////////////////////////////////////////////////////////////
//  impls                                                                    //
///////////////////////////////////////////////////////////////////////////////

impl SdlGliumDisplayFacade {
  /// &#9888; **Warning**: the returned window reference does *not* contain a
  /// "real" reference to the `sdl2::VideoSubsystem`. While most methods should
  /// work, it is not possible to create a window from a thread other than the
  /// main thread, so the `sdl2::VideoSubsystem::window` function to build a
  /// new window **should not be called**.
  pub unsafe fn window (&self) -> &sdl2::video::Window {
    let ptr = self.sdl_window_impostor.get();
    let window : &sdl2::video::Window = std::mem::transmute (ptr);
    window
  }

  /// &#9888; **Warning**: the returned window reference does *not* contain a
  /// "real" reference to the `sdl2::VideoSubsystem`. While most methods should
  /// work, it is not possible to create a window from a thread other than the
  /// main thread, so the `sdl2::VideoSubsystem::window` function to build a
  /// new window **should not be called**.
  pub unsafe fn window_mut (&mut self) -> &mut sdl2::video::Window {
    let ptr = self.sdl_window_impostor.get();
    let window : &mut sdl2::video::Window = std::mem::transmute (ptr);
    window
  }

  /// Start drawing on the backbuffer.
  ///
  /// This function returns a `Frame`, which can be used to draw on it.  When
  /// the `Frame` is destroyed, the buffers are swapped.
  ///
  /// Note that destroying a `Frame` is immediate, even if vsync is enabled.
  pub fn draw (&self) -> glium::Frame {
    use glium::backend::Backend;
    glium::Frame::new (
      self.glium_context.clone(),
      self.window_backend.get_framebuffer_dimensions())
  }
}

impl SdlGlWindowBackend {
  /// Build Glium with current context checks and with default debug callback
  /// behavior.
  pub fn build_glium (self)
    -> Result <SdlGliumDisplayFacade, glium::IncompatibleOpenGl>
  {
    self.build_glium_debug (Default::default())
  }

  /// Build Glium without current context checks and with default debug
  /// callback behavior.
  pub fn build_glium_unchecked (self)
    -> Result <SdlGliumDisplayFacade, glium::IncompatibleOpenGl>
  {
    self.build_glium_unchecked_debug (Default::default())
  }

  /// Build Glium with current context checks and with the given debug callback
  /// behavior.
  pub fn build_glium_debug (mut self,
    debug : glium::debug::DebugCallbackBehavior
  ) -> Result <SdlGliumDisplayFacade, glium::IncompatibleOpenGl> {
    let gl_funs = self.gl_funs.take().unwrap();
    let sdl_window_context_impostor
      = SdlWindowContextImpostor::new (self.window_raw.as_ptr());
    let sdl_window_impostor = std::rc::Rc::new (std::cell::UnsafeCell::new (
      SdlWindowImpostor::new (sdl_window_context_impostor)));
    let window_backend = std::rc::Rc::new (self);
    let glium_context = try!{
      unsafe {
        glium::backend::Context::new_hack (
          window_backend.clone(),
          *gl_funs,
          true,
          debug
        )
      }
    };
    Ok (SdlGliumDisplayFacade {
      glium_context,
      window_backend,
      sdl_window_impostor
    })
  }

  /// Build Glium without current context checks and with the given debug
  /// callback behavior.
  pub fn build_glium_unchecked_debug (mut self,
    debug : glium::debug::DebugCallbackBehavior
  ) -> Result <SdlGliumDisplayFacade, glium::IncompatibleOpenGl> {
    let gl_funs = self.gl_funs.take().unwrap();
    let sdl_window_context_impostor
      = SdlWindowContextImpostor::new (self.window_raw.as_ptr());
    let sdl_window_impostor = std::rc::Rc::new (std::cell::UnsafeCell::new (
      SdlWindowImpostor::new (sdl_window_context_impostor)));
    let window_backend = std::rc::Rc::new (self);
    let glium_context = try!{
      unsafe {
        glium::backend::Context::new_hack (
          window_backend.clone(),
          *gl_funs,
          false,
          debug
        )
      }
    };
    Ok (SdlGliumDisplayFacade {
      glium_context,
      window_backend,
      sdl_window_impostor
    })
  }

} // end impl SdlGlWindowBackend

/// Implementation of drop will destroy the window and delete the OpenGL
/// context.
///
/// NB: Because the Glium backend context holds a reference to this structure,
/// it should be guaranteed not to drop while a reference to the Glium context
/// exists (such as in a `glium::Frame` object). Also, because the lifetime of
/// any `&sdl2::video::Window` reference returned from the display is tied to
/// the display itself, it is not possible for this to drop while any window
/// references are in scope.
impl Drop for SdlGlWindowBackend {
  fn drop (&mut self) {
    unsafe { sdl2_sys::SDL_DestroyWindow (self.window_raw.as_ptr()) };
    unsafe { sdl2_sys::SDL_GL_DeleteContext (self.gl_context_raw.as_ptr()) };
  }
}

/// Backend implementation basically follows that of the `glium-sdl2` crate,
/// except with raw `SDL_GL_*` calls.
unsafe impl glium::backend::Backend for SdlGlWindowBackend {
  fn swap_buffers (&self) -> Result<(), glium::SwapBuffersError> {
    // TODO: is context loss is possible?
    unsafe { sdl2_sys::SDL_GL_SwapWindow (self.window_raw.as_ptr()) }
    Ok(())
  }

  unsafe fn get_proc_address (&self, symbol : &str)
    -> *const std::os::raw::c_void
  {
    match std::ffi::CString::new (symbol) {
      Ok (symbol) => {
        sdl2_sys::SDL_GL_GetProcAddress (
          symbol.as_ptr() as *const std::os::raw::c_char
        ) as *const std::os::raw::c_void
      }
      Err (_) => std::ptr::null()
    }
  }

  fn get_framebuffer_dimensions (&self) -> (u32, u32) {
    let mut width  : std::os::raw::c_int = 0;
    let mut height : std::os::raw::c_int = 0;
    unsafe {
      sdl2_sys::SDL_GL_GetDrawableSize (
        self.window_raw.as_ptr(), &mut width, &mut height) };
    (width as u32, height as u32)
  }

  fn is_current (&self) -> bool {
    let current_raw = unsafe { sdl2_sys::SDL_GL_GetCurrentContext() };
    self.gl_context_raw.as_ptr() == current_raw
  }

  unsafe fn make_current (&self) {
    let result = if 0 == sdl2_sys::SDL_GL_MakeCurrent (
      self.window_raw.as_ptr(), self.gl_context_raw.as_ptr()
    ) {
      Ok (())
    } else {
      Err (sdl2::get_error())
    };
    result.unwrap();
  }
}

impl SdlGlWindowBuilder for sdl2::video::WindowBuilder {
  /// Builds a raw window backend and releases the context.
  ///
  /// # Panics
  ///
  /// Call will panic if the size of the `sdl2::video::Window` type does not
  /// match the size of the internal `SdlWindowImpostor` type, or if the
  /// `sdl2::video::WindowContext` type does not match the size of the internal
  /// `SdlWindowContextImpostor` type.
  ///
  /// TODO: can this be made a compile time check when compile-time assertions
  /// are allowed ?
  fn build_backend (&mut self) -> Result <SdlGlWindowBackend, BackendBuildError> {
    assert_eq!(
      std::mem::size_of::<sdl2::video::Window>(),
      std::mem::size_of::<SdlWindowImpostor>());
    assert_eq!(
      std::mem::size_of::<sdl2::video::WindowContext>(),
      std::mem::size_of::<SdlWindowContextImpostor>());

    use glium::backend::Backend;

    // opengl must be requested
    self.opengl();
    // create window from self
    let (window_raw, video_subsystem) = unsafe {
      let (window_raw, video_subsystem) = try!{ self.build_hack() };
      (std::ptr::Unique::new_unchecked (window_raw), video_subsystem)
    };
    // create gl context
    let gl_context_raw = unsafe {
      let gl_context_raw : sdl2_sys::SDL_GLContext
        = sdl2_sys::SDL_GL_CreateContext (window_raw.as_ptr());
      if gl_context_raw.is_null() {
        return Err (BackendBuildError::ContextCreationError (sdl2::get_error()))
      }
      std::ptr::Unique::new_unchecked (gl_context_raw)
    };
    let mut window_backend
      = SdlGlWindowBackend { window_raw, gl_context_raw, gl_funs: None };
    // load gl function pointers
    window_backend.gl_funs = Some (Box::new (glium::gl::Gl::load_with (
      |symbol| unsafe { window_backend.get_proc_address (symbol) as *const _ }
    )));

    video_subsystem.gl_release_current_context().unwrap();

    Ok (window_backend)
  }
}

impl From <sdl2::video::WindowBuildError> for BackendBuildError {
  fn from (err : sdl2::video::WindowBuildError) -> Self {
    BackendBuildError::WindowBuildError (err)
  }
}

impl From <String> for BackendBuildError {
  fn from (err : String) -> Self {
    BackendBuildError::ContextCreationError (err)
  }
}

impl SdlWindowImpostor {
  fn new (window_context_impostor : SdlWindowContextImpostor) -> Self {
    SdlWindowImpostor {
      window_context_impostor: std::rc::Rc::new (window_context_impostor)
    }
  }
}

impl SdlWindowContextImpostor {
  fn new (window_raw : *mut sdl2_sys::SDL_Window) -> Self {
    SdlWindowContextImpostor {
      _video_subsystem: std::rc::Rc::new (()),
      _window_raw:      window_raw
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;
  /// TODO: check offset of transmuted values ?
  #[test]
  fn test() {
    assert_eq!(
      std::mem::size_of::<sdl2::video::Window>(),
      std::mem::size_of::<SdlWindowImpostor>());
    assert_eq!(
      std::mem::size_of::<sdl2::video::WindowContext>(),
      std::mem::size_of::<SdlWindowContextImpostor>());
  }
}
