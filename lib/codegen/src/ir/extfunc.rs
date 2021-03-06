//! External function calls.
//!
//! To a Cretonne function, all functions are "external". Directly called functions must be
//! declared in the preamble, and all function calls must have a signature.
//!
//! This module declares the data types used to represent external functions and call signatures.

use ir::{ArgumentLoc, ExternalName, SigRef, Type};
use isa::{RegInfo, RegUnit};
use settings::CallConv;
use std::cmp;
use std::fmt;
use std::str::FromStr;
use std::vec::Vec;

/// Function signature.
///
/// The function signature describes the types of formal parameters and return values along with
/// other details that are needed to call a function correctly.
///
/// A signature can optionally include ISA-specific ABI information which specifies exactly how
/// arguments and return values are passed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Signature {
    /// The arguments passed to the function.
    pub params: Vec<AbiParam>,
    /// Values returned from the function.
    pub returns: Vec<AbiParam>,

    /// Calling convention.
    pub call_conv: CallConv,

    /// When the signature has been legalized to a specific ISA, this holds the size of the
    /// argument array on the stack. Before legalization, this is `None`.
    ///
    /// This can be computed from the legalized `params` array as the maximum (offset plus
    /// byte size) of the `ArgumentLoc::Stack(offset)` argument.
    pub argument_bytes: Option<u32>,
}

impl Signature {
    /// Create a new blank signature.
    pub fn new(call_conv: CallConv) -> Self {
        Self {
            params: Vec::new(),
            returns: Vec::new(),
            call_conv,
            argument_bytes: None,
        }
    }

    /// Clear the signature so it is identical to a fresh one returned by `new()`.
    pub fn clear(&mut self, call_conv: CallConv) {
        self.params.clear();
        self.returns.clear();
        self.call_conv = call_conv;
        self.argument_bytes = None;
    }

    /// Compute the size of the stack arguments and mark signature as legalized.
    ///
    /// Even if there are no stack arguments, this will set `params` to `Some(0)` instead
    /// of `None`. This indicates that the signature has been legalized.
    pub fn compute_argument_bytes(&mut self) {
        let bytes = self.params
            .iter()
            .filter_map(|arg| match arg.location {
                ArgumentLoc::Stack(offset) if offset >= 0 => {
                    Some(offset as u32 + arg.value_type.bytes())
                }
                _ => None,
            })
            .fold(0, cmp::max);
        self.argument_bytes = Some(bytes);
    }

    /// Return an object that can display `self` with correct register names.
    pub fn display<'a, R: Into<Option<&'a RegInfo>>>(&'a self, regs: R) -> DisplaySignature<'a> {
        DisplaySignature(self, regs.into())
    }

    /// Find the index of a presumed unique special-purpose parameter.
    pub fn special_param_index(&self, purpose: ArgumentPurpose) -> Option<usize> {
        self.params.iter().rposition(|arg| arg.purpose == purpose)
    }
}

/// Wrapper type capable of displaying a `Signature` with correct register names.
pub struct DisplaySignature<'a>(&'a Signature, Option<&'a RegInfo>);

fn write_list(f: &mut fmt::Formatter, args: &[AbiParam], regs: Option<&RegInfo>) -> fmt::Result {
    match args.split_first() {
        None => {}
        Some((first, rest)) => {
            write!(f, "{}", first.display(regs))?;
            for arg in rest {
                write!(f, ", {}", arg.display(regs))?;
            }
        }
    }
    Ok(())
}

impl<'a> fmt::Display for DisplaySignature<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(")?;
        write_list(f, &self.0.params, self.1)?;
        write!(f, ")")?;
        if !self.0.returns.is_empty() {
            write!(f, " -> ")?;
            write_list(f, &self.0.returns, self.1)?;
        }
        write!(f, " {}", self.0.call_conv)
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.display(None).fmt(f)
    }
}

/// Function parameter or return value descriptor.
///
/// This describes the value type being passed to or from a function along with flags that affect
/// how the argument is passed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AbiParam {
    /// Type of the argument value.
    pub value_type: Type,
    /// Special purpose of argument, or `Normal`.
    pub purpose: ArgumentPurpose,
    /// Method for extending argument to a full register.
    pub extension: ArgumentExtension,

    /// ABI-specific location of this argument, or `Unassigned` for arguments that have not yet
    /// been legalized.
    pub location: ArgumentLoc,
}

impl AbiParam {
    /// Create a parameter with default flags.
    pub fn new(vt: Type) -> Self {
        Self {
            value_type: vt,
            extension: ArgumentExtension::None,
            purpose: ArgumentPurpose::Normal,
            location: Default::default(),
        }
    }

    /// Create a special-purpose parameter that is not (yet) bound to a specific register.
    pub fn special(vt: Type, purpose: ArgumentPurpose) -> Self {
        Self {
            value_type: vt,
            extension: ArgumentExtension::None,
            purpose,
            location: Default::default(),
        }
    }

    /// Create a parameter for a special-purpose register.
    pub fn special_reg(vt: Type, purpose: ArgumentPurpose, regunit: RegUnit) -> Self {
        Self {
            value_type: vt,
            extension: ArgumentExtension::None,
            purpose,
            location: ArgumentLoc::Reg(regunit),
        }
    }

    /// Convert `self` to a parameter with the `uext` flag set.
    pub fn uext(self) -> Self {
        debug_assert!(self.value_type.is_int(), "uext on {} arg", self.value_type);
        Self {
            extension: ArgumentExtension::Uext,
            ..self
        }
    }

    /// Convert `self` to a parameter type with the `sext` flag set.
    pub fn sext(self) -> Self {
        debug_assert!(self.value_type.is_int(), "sext on {} arg", self.value_type);
        Self {
            extension: ArgumentExtension::Sext,
            ..self
        }
    }

    /// Return an object that can display `self` with correct register names.
    pub fn display<'a, R: Into<Option<&'a RegInfo>>>(&'a self, regs: R) -> DisplayAbiParam<'a> {
        DisplayAbiParam(self, regs.into())
    }
}

/// Wrapper type capable of displaying a `AbiParam` with correct register names.
pub struct DisplayAbiParam<'a>(&'a AbiParam, Option<&'a RegInfo>);

impl<'a> fmt::Display for DisplayAbiParam<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0.value_type)?;
        match self.0.extension {
            ArgumentExtension::None => {}
            ArgumentExtension::Uext => write!(f, " uext")?,
            ArgumentExtension::Sext => write!(f, " sext")?,
        }
        if self.0.purpose != ArgumentPurpose::Normal {
            write!(f, " {}", self.0.purpose)?;
        }

        if self.0.location.is_assigned() {
            write!(f, " [{}]", self.0.location.display(self.1))?;
        }

        Ok(())
    }
}

impl fmt::Display for AbiParam {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.display(None).fmt(f)
    }
}

/// Function argument extension options.
///
/// On some architectures, small integer function arguments are extended to the width of a
/// general-purpose register.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum ArgumentExtension {
    /// No extension, high bits are indeterminate.
    None,
    /// Unsigned extension: high bits in register are 0.
    Uext,
    /// Signed extension: high bits in register replicate sign bit.
    Sext,
}

/// The special purpose of a function argument.
///
/// Function arguments and return values are used to pass user program values between functions,
/// but they are also used to represent special registers with significance to the ABI such as
/// frame pointers and callee-saved registers.
///
/// The argument purpose is used to indicate any special meaning of an argument or return value.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum ArgumentPurpose {
    /// A normal user program value passed to or from a function.
    Normal,

    /// Struct return pointer.
    ///
    /// When a function needs to return more data than will fit in registers, the caller passes a
    /// pointer to a memory location where the return value can be written. In some ABIs, this
    /// struct return pointer is passed in a specific register.
    ///
    /// This argument kind can also appear as a return value for ABIs that require a function with
    /// a `StructReturn` pointer argument to also return that pointer in a register.
    StructReturn,

    /// The link register.
    ///
    /// Most RISC architectures implement calls by saving the return address in a designated
    /// register rather than pushing it on the stack. This is represented with a `Link` argument.
    ///
    /// Similarly, some return instructions expect the return address in a register represented as
    /// a `Link` return value.
    Link,

    /// The frame pointer.
    ///
    /// This indicates the frame pointer register which has a special meaning in some ABIs.
    ///
    /// The frame pointer appears as an argument and as a return value since it is a callee-saved
    /// register.
    FramePointer,

    /// A callee-saved register.
    ///
    /// Some calling conventions have registers that must be saved by the callee. These registers
    /// are represented as `CalleeSaved` arguments and return values.
    CalleeSaved,

    /// A VM context pointer.
    ///
    /// This is a pointer to a context struct containing details about the current sandbox. It is
    /// used as a base pointer for `vmctx` global variables.
    VMContext,

    /// A signature identifier.
    ///
    /// This is a special-purpose argument used to identify the calling convention expected by the
    /// caller in an indirect call. The callee can verify that the expected signature ID matches.
    SignatureId,
}

/// Text format names of the `ArgumentPurpose` variants.
static PURPOSE_NAMES: [&str; 7] = ["normal", "sret", "link", "fp", "csr", "vmctx", "sigid"];

impl fmt::Display for ArgumentPurpose {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(PURPOSE_NAMES[*self as usize])
    }
}

impl FromStr for ArgumentPurpose {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "normal" => Ok(ArgumentPurpose::Normal),
            "sret" => Ok(ArgumentPurpose::StructReturn),
            "link" => Ok(ArgumentPurpose::Link),
            "fp" => Ok(ArgumentPurpose::FramePointer),
            "csr" => Ok(ArgumentPurpose::CalleeSaved),
            "vmctx" => Ok(ArgumentPurpose::VMContext),
            "sigid" => Ok(ArgumentPurpose::SignatureId),
            _ => Err(()),
        }
    }
}

/// An external function.
///
/// Information about a function that can be called directly with a direct `call` instruction.
#[derive(Clone, Debug)]
pub struct ExtFuncData {
    /// Name of the external function.
    pub name: ExternalName,
    /// Call signature of function.
    pub signature: SigRef,
    /// Will this function be defined nearby, such that it will always be a certain distance away,
    /// after linking? If so, references to it can avoid going through a GOT or PLT. Note that
    /// symbols meant to be preemptible cannot be considered colocated.
    pub colocated: bool,
}

impl fmt::Display for ExtFuncData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.colocated {
            write!(f, "colocated ")?;
        }
        write!(f, "{} {}", self.name, self.signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::types::{B8, F32, I32};
    use std::string::ToString;

    #[test]
    fn argument_type() {
        let t = AbiParam::new(I32);
        assert_eq!(t.to_string(), "i32");
        let mut t = t.uext();
        assert_eq!(t.to_string(), "i32 uext");
        assert_eq!(t.sext().to_string(), "i32 sext");
        t.purpose = ArgumentPurpose::StructReturn;
        assert_eq!(t.to_string(), "i32 uext sret");
    }

    #[test]
    fn argument_purpose() {
        let all_purpose = [
            ArgumentPurpose::Normal,
            ArgumentPurpose::StructReturn,
            ArgumentPurpose::Link,
            ArgumentPurpose::FramePointer,
            ArgumentPurpose::CalleeSaved,
            ArgumentPurpose::VMContext,
        ];
        for (&e, &n) in all_purpose.iter().zip(PURPOSE_NAMES.iter()) {
            assert_eq!(e.to_string(), n);
            assert_eq!(Ok(e), n.parse());
        }
    }

    #[test]
    fn call_conv() {
        for &cc in &[
            CallConv::Fast,
            CallConv::Cold,
            CallConv::SystemV,
            CallConv::Fastcall,
            CallConv::Baldrdash,
        ]
        {
            assert_eq!(Ok(cc), cc.to_string().parse())
        }
    }

    #[test]
    fn signatures() {
        let mut sig = Signature::new(CallConv::Baldrdash);
        assert_eq!(sig.to_string(), "() baldrdash");
        sig.params.push(AbiParam::new(I32));
        assert_eq!(sig.to_string(), "(i32) baldrdash");
        sig.returns.push(AbiParam::new(F32));
        assert_eq!(sig.to_string(), "(i32) -> f32 baldrdash");
        sig.params.push(AbiParam::new(I32.by(4).unwrap()));
        assert_eq!(sig.to_string(), "(i32, i32x4) -> f32 baldrdash");
        sig.returns.push(AbiParam::new(B8));
        assert_eq!(sig.to_string(), "(i32, i32x4) -> f32, b8 baldrdash");

        // Test the offset computation algorithm.
        assert_eq!(sig.argument_bytes, None);
        sig.params[1].location = ArgumentLoc::Stack(8);
        sig.compute_argument_bytes();
        // An `i32x4` at offset 8 requires a 24-byte argument array.
        assert_eq!(sig.argument_bytes, Some(24));
        // Order does not matter.
        sig.params[0].location = ArgumentLoc::Stack(24);
        sig.compute_argument_bytes();
        assert_eq!(sig.argument_bytes, Some(28));

        // Writing ABI-annotated signatures.
        assert_eq!(
            sig.to_string(),
            "(i32 [24], i32x4 [8]) -> f32, b8 baldrdash"
        );
    }
}
