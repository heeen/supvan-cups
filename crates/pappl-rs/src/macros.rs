//! Internal declarative macros for generating builder boilerplate.

/// Generate a builder struct with chainable setters and an `apply` method
/// that writes the simple fields into a target C struct.
///
/// # Field kinds (macro-managed)
///
/// - `scalar $field: $ty` — direct assignment with `as _` cast
/// - `bool_ $field: bool` — direct bool assignment
/// - `c_str $field: &'static CStr` — stores the CStr, applies `.as_ptr()`
/// - `buf $field: &'a [u8]` — copies bytes into a `[c_char; N]` buffer
///
/// # Extras block
///
/// Fields in the `extras { ... }` block are included in the struct with
/// a default value but get no generated setter or apply logic — those
/// are hand-written in a separate `impl` block.
macro_rules! pappl_builder {
    (
        $(#[$meta:meta])*
        pub struct $Builder:ident => $Target:ty {
            $(
                $(#[$fmeta:meta])*
                $kind:ident $field:ident : $rust_ty:ty
            ),* $(,)?
        }
        extras {
            $(
                $(#[$emeta:meta])*
                $efield:ident : $ety:ty = $einit:expr
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        pub struct $Builder<'a> {
            $(
                $(#[$fmeta])*
                $field: Option<$rust_ty>,
            )*
            $(
                $(#[$emeta])*
                pub(crate) $efield: $ety,
            )*
            _phantom: std::marker::PhantomData<&'a ()>,
        }

        impl<'a> $Builder<'a> {
            pub fn new() -> Self {
                Self {
                    $( $field: None, )*
                    $( $efield: $einit, )*
                    _phantom: std::marker::PhantomData,
                }
            }

            $(
                $(#[$fmeta])*
                pub fn $field(mut self, v: $rust_ty) -> Self {
                    self.$field = Some(v);
                    self
                }
            )*

            /// Apply macro-managed fields to the target struct.
            unsafe fn apply_simple(&self, target: &mut $Target) {
                $(
                    pappl_builder!(@apply_field $kind, target, $field, self.$field);
                )*
            }
        }

        impl<'a> Default for $Builder<'a> {
            fn default() -> Self {
                Self::new()
            }
        }
    };

    // --- per-kind apply arms ---

    (@apply_field scalar, $target:ident, $field:ident, $val:expr) => {
        if let Some(v) = $val {
            $target.$field = v as _;
        }
    };

    (@apply_field bool_, $target:ident, $field:ident, $val:expr) => {
        if let Some(v) = $val {
            $target.$field = v;
        }
    };

    (@apply_field c_str, $target:ident, $field:ident, $val:expr) => {
        if let Some(v) = $val {
            $target.$field = v.as_ptr();
        }
    };

    (@apply_field buf, $target:ident, $field:ident, $val:expr) => {
        if let Some(v) = $val {
            $crate::util::copy_to_c_buf(&mut $target.$field, v);
        }
    };
}

pub(crate) use pappl_builder;
