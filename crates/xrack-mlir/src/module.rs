use crate::ffi;
use std::{
    cell::Cell,
    collections::BTreeSet,
    ffi::c_void,
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};
use xrack_syntax::ast::{Parameter, Span, Type};

struct Context(ffi::MlirContext);

impl Context {
    fn new() -> Self {
        // Diagnostics are collected synchronously on the calling thread.
        let context = Self(unsafe { ffi::mlirContextCreateWithThreading(false) });
        unsafe { ffi::mlirDialectHandleLoadDialect(ffi::mlirGetDialectHandle__xr__(), context.0) };
        context
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // The module is destroyed before its context.
        unsafe { ffi::mlirContextDestroy(self.0) };
    }
}

struct PassManager(ffi::MlirPassManager);

impl Drop for PassManager {
    fn drop(&mut self) {
        // The manager is scoped to the call that borrows its context.
        unsafe { ffi::mlirPassManagerDestroy(self.0) };
    }
}

/// An owned MLIR module. Read-only handles borrow it and cannot survive a pass run.
pub struct Module {
    raw: ffi::MlirModule,
    context: Context,
    verified: Cell<bool>,
    visual_cache_plan: bool,
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Module").finish_non_exhaustive()
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        // The module is owned by Rust. Its context remains alive here.
        unsafe { ffi::mlirModuleDestroy(self.raw) };
    }
}

impl Module {
    pub(crate) fn empty() -> Self {
        let context = Context::new();
        let raw = unsafe { ffi::mlirModuleCreateEmpty(ffi::mlirLocationUnknownGet(context.0)) };
        Self {
            raw,
            context,
            verified: Cell::new(false),
            visual_cache_plan: false,
        }
    }

    pub(crate) fn builder_handles(&mut self) -> (ffi::MlirContext, ffi::MlirBlock) {
        self.verified.set(false);
        (self.context.0, unsafe { ffi::mlirModuleGetBody(self.raw) })
    }

    /// Parse and verify XR IR without running its optimization pipeline.
    pub fn parse(source: &str) -> Result<Self, String> {
        let context = Context::new();
        let raw = capture_diagnostics(&context, || {
            // The parser borrows source only for the duration of the call.
            let raw = unsafe { ffi::mlirModuleCreateParse(context.0, string_ref(source)) };
            if raw.ptr.is_null() {
                return Err("failed to parse MLIR module");
            }
            Ok(raw)
        })?;
        let mut module = Self {
            raw,
            context,
            verified: Cell::new(false),
            visual_cache_plan: false,
        };
        module.verify()?;
        module.normalize_element_keys();
        module.verify()?;
        Ok(module)
    }

    fn normalize_element_keys(&mut self) {
        fn collect<'m>(
            parent: OperationRef<'m>,
            used: &mut BTreeSet<String>,
            missing_keys: &mut Vec<OperationRef<'m>>,
            root_to_normalize: &mut Option<OperationRef<'m>>,
        ) {
            for op in parent.children() {
                if !matches!(op.name(), "xr.element" | "xr.instance") {
                    continue;
                }
                let key = op.attribute("key").or_else(|| op.attribute("id"));
                let is_root = parent.name() == "xr.component";
                if is_root {
                    // The dialect resolves root references through the component.
                    if key != Some("root") {
                        *root_to_normalize = Some(op);
                    }
                } else if let Some(key) = key.filter(|key| !key.is_empty()) {
                    used.insert(key.to_owned());
                } else {
                    missing_keys.push(op);
                }
                collect(op, used, missing_keys, root_to_normalize);
            }
        }

        for component in self.operations().filter(|op| op.name() == "xr.component") {
            let mut used = BTreeSet::from(["root".to_owned()]);
            let mut missing_keys = Vec::new();
            let mut root_to_normalize = None;
            collect(
                component,
                &mut used,
                &mut missing_keys,
                &mut root_to_normalize,
            );
            let candidates = (0..).map(|index| format!("__xr_anon_{index}"));
            let generated = missing_keys
                .into_iter()
                .zip(candidates.filter(|candidate| !used.contains(candidate)));
            for (op, key) in root_to_normalize
                .into_iter()
                .map(|op| (op, "root".to_owned()))
                .chain(generated)
            {
                self.verified.set(false);
                let selector = op
                    .children()
                    .find(|child| child.name() == "xr.state_select");
                // The key and the local selector's owner are verified together.
                // Both attributes belong to the module and are updated before it is shared.
                unsafe {
                    let value = ffi::mlirStringAttrGet(self.context.0, string_ref(&key));
                    ffi::mlirOperationSetInherentAttributeByName(op.raw, string_ref("key"), value);
                    if let Some(selector) = selector {
                        ffi::mlirOperationSetInherentAttributeByName(
                            selector.raw,
                            string_ref("owner"),
                            value,
                        );
                    }
                }
            }
        }
    }

    /// Verify the ODS operations and XR's module-wide constraints.
    pub fn verify(&self) -> Result<(), String> {
        if self.verified.get() {
            return Ok(());
        }
        capture_diagnostics(&self.context, || {
            let op = unsafe { ffi::mlirModuleGetOperation(self.raw) };
            if !unsafe { ffi::mlirOperationVerify(op) }
                || !unsafe { ffi::xrackMlirVerifyModule(self.raw) }
            {
                return Err("XR module verification failed");
            }
            Ok(())
        })?;
        crate::visual::verify_element_interfaces(self)?;
        self.verified.set(true);
        Ok(())
    }

    /// Consume the module before running mutating passes, invalidating old borrows.
    pub fn optimize(mut self) -> Result<Self, String> {
        self.verify()?;
        self.verified.set(false);
        capture_diagnostics(&self.context, || {
            let manager = PassManager(unsafe { ffi::mlirPassManagerCreate(self.context.0) });
            // The operation, manager and dialect all belong to this context.
            unsafe {
                ffi::mlirPassManagerEnableVerifier(manager.0, true);
                ffi::xrackMlirAddPipeline(manager.0);
                let result =
                    ffi::mlirPassManagerRunOnOp(manager.0, ffi::mlirModuleGetOperation(self.raw));
                // Mirrors mlirLogicalResultIsSuccess, a static inline C helper.
                if result.value == 0 {
                    return Err("XR optimization pipeline failed");
                }
                if !ffi::xrackMlirVerifyModule(self.raw) {
                    return Err("optimized XR module verification failed");
                }
            }
            Ok(())
        })?;
        crate::visual::verify_element_interfaces(&self)?;
        self.verified.set(true);
        self.visual_cache_plan = true;
        Ok(self)
    }

    /// Only use visual annotations created by this module's optimization pipeline.
    pub fn has_visual_cache_plan(&self) -> bool {
        self.visual_cache_plan
    }

    pub fn operations(&self) -> impl Iterator<Item = OperationRef<'_>> {
        // The module body always exists and is owned by the module.
        BlockRef {
            raw: unsafe { ffi::mlirModuleGetBody(self.raw) },
            owner: PhantomData,
        }
        .operations()
    }

    pub fn to_mlir(&self) -> String {
        let mut output = Vec::new();
        // Printing invokes the callback synchronously and does not mutate IR.
        unsafe {
            let flags = ffi::mlirOpPrintingFlagsCreate();
            ffi::mlirOpPrintingFlagsEnableDebugInfo(flags, true, false);
            ffi::mlirOperationPrintWithFlags(
                ffi::mlirModuleGetOperation(self.raw),
                flags,
                Some(append_bytes),
                data(&mut output),
            );
            ffi::mlirOpPrintingFlagsDestroy(flags);
        }
        String::from_utf8_lossy(&output).into_owned()
    }
}

pub fn process(source: &str) -> Result<Module, String> {
    Module::parse(source)?.optimize()
}

/// Decode the finite floating-point spellings accepted by the XR dialect.
pub fn parse_float_literal(spelling: &str) -> Option<f64> {
    let mut value = 0.0;
    let parsed = unsafe { ffi::xrackMlirParseFloatLiteral(string_ref(spelling), &mut value) };
    parsed.then_some(value)
}

pub(crate) fn string_ref(text: &str) -> ffi::MlirStringRef {
    ffi::MlirStringRef {
        data: text.as_ptr().cast(),
        length: text.len(),
    }
}

// Caller supplies the lifetime of the native owner of this string.
unsafe fn bytes<'a>(text: ffi::MlirStringRef) -> &'a [u8] {
    if text.length == 0 {
        return &[];
    }
    // A nonempty MlirStringRef points to initialized bytes owned by MLIR.
    unsafe { std::slice::from_raw_parts(text.data.cast(), text.length) }
}

fn data(output: &mut Vec<u8>) -> *mut c_void {
    (output as *mut Vec<u8>).cast()
}

unsafe extern "C" fn append_bytes(text: ffi::MlirStringRef, output: *mut c_void) {
    // The caller supplies an exclusive, live buffer for this synchronous callback.
    unsafe { &mut *output.cast::<Vec<u8>>() }.extend_from_slice(unsafe { bytes(text) });
}

unsafe extern "C" fn collect_diagnostic(
    diag: ffi::MlirDiagnostic,
    output: *mut c_void,
) -> ffi::MlirLogicalResult {
    // Printing consumes the diagnostic before the handler returns.
    unsafe { ffi::mlirDiagnosticPrint(diag, Some(append_bytes), output) };
    unsafe { &mut *output.cast::<Vec<u8>>() }.push(b'\n');
    // Mirrors the inline mlirLogicalResultSuccess helper.
    ffi::MlirLogicalResult { value: 1 }
}

fn capture_diagnostics<T>(
    context: &Context,
    call: impl FnOnce() -> Result<T, &'static str>,
) -> Result<T, String> {
    struct Attachment<'a> {
        context: &'a Context,
        id: ffi::MlirDiagnosticHandlerID,
        buffer: *mut Vec<u8>,
    }
    impl Drop for Attachment<'_> {
        fn drop(&mut self) {
            // Detach before releasing the callback's user data.
            unsafe {
                ffi::mlirContextDetachDiagnosticHandler(self.context.0, self.id);
                drop(Box::from_raw(self.buffer));
            }
        }
    }
    let buffer = Box::into_raw(Box::new(Vec::<u8>::new()));
    // The attachment owns this allocation until the callback is detached.
    let id = unsafe {
        ffi::mlirContextAttachDiagnosticHandler(
            context.0,
            Some(collect_diagnostic),
            buffer.cast(),
            None,
        )
    };
    let attachment = Attachment {
        context,
        id,
        buffer,
    };
    call().map_err(|fallback| {
        // The synchronous operation and all its diagnostic callbacks have returned.
        let diagnostics = unsafe { &*attachment.buffer };
        if diagnostics.is_empty() {
            fallback.to_owned()
        } else {
            String::from_utf8_lossy(diagnostics).into_owned()
        }
    })
}

#[derive(Clone, Copy, Debug)]
pub struct OperationRef<'m> {
    raw: ffi::MlirOperation,
    owner: PhantomData<&'m Module>,
}

impl<'m> OperationRef<'m> {
    fn from_raw(raw: ffi::MlirOperation) -> Option<Self> {
        (!raw.ptr.is_null()).then_some(Self {
            raw,
            owner: PhantomData,
        })
    }

    pub fn name(self) -> &'m str {
        // Registered operation identifiers are valid UTF-8 and live in the context.
        let name = unsafe { ffi::mlirIdentifierStr(ffi::mlirOperationGetName(self.raw)) };
        std::str::from_utf8(unsafe { bytes(name) }).expect("MLIR operation name is UTF-8")
    }

    pub fn attribute(self, name: &str) -> Option<&'m str> {
        // String and symbol reference attributes are immutable context-owned values.
        let attr = unsafe { ffi::mlirOperationGetAttributeByName(self.raw, string_ref(name)) };
        if attr.ptr.is_null() {
            return None;
        }
        let text = unsafe {
            if ffi::mlirAttributeIsAString(attr) {
                ffi::mlirStringAttrGetValue(attr)
            } else if ffi::mlirAttributeIsAFlatSymbolRef(attr) {
                ffi::mlirFlatSymbolRefAttrGetValue(attr)
            } else {
                return None;
            }
        };
        std::str::from_utf8(unsafe { bytes(text) }).ok()
    }

    pub fn attr(self, name: &str) -> Result<&'m str, String> {
        self.attribute(name)
            .ok_or_else(|| format!("{} missing or invalid `{name}`", self.name()))
    }

    /// Decode the signature stored by an XR callback or event handler.
    pub fn parameters(self) -> Result<Vec<Parameter>, String> {
        serde_json::from_str(self.attr("parameters")?).map_err(|error| error.to_string())
    }

    pub fn string_array_attribute(self, name: &str) -> Option<Vec<&'m str>> {
        // The array and its strings are immutable and owned by the module context.
        let attr = unsafe { ffi::mlirOperationGetAttributeByName(self.raw, string_ref(name)) };
        if attr.ptr.is_null() || !unsafe { ffi::mlirAttributeIsAArray(attr) } {
            return None;
        }
        (0..unsafe { ffi::mlirArrayAttrGetNumElements(attr) })
            .map(|index| {
                let element = unsafe { ffi::mlirArrayAttrGetElement(attr, index) };
                if !unsafe { ffi::mlirAttributeIsAString(element) } {
                    return None;
                }
                let value = unsafe { ffi::mlirStringAttrGetValue(element) };
                std::str::from_utf8(unsafe { bytes(value) }).ok()
            })
            .collect()
    }

    pub fn integer_attribute(self, name: &str) -> Option<i64> {
        // Only read the value of a valid, context-owned integer attribute.
        let attr = unsafe { ffi::mlirOperationGetAttributeByName(self.raw, string_ref(name)) };
        if attr.ptr.is_null() || !unsafe { ffi::mlirAttributeIsAInteger(attr) } {
            return None;
        }
        let ty = unsafe { ffi::mlirAttributeGetType(attr) };
        if !unsafe { ffi::mlirTypeIsAInteger(ty) }
            || unsafe { ffi::mlirIntegerTypeGetWidth(ty) } > 64
        {
            return None;
        }
        Some(unsafe { ffi::mlirIntegerAttrGetValueInt(attr) })
    }

    pub fn i64_array_attribute(self, name: &str) -> Option<Vec<i64>> {
        let attr = unsafe { ffi::mlirOperationGetAttributeByName(self.raw, string_ref(name)) };
        if attr.ptr.is_null() || !unsafe { ffi::mlirAttributeIsADenseI64Array(attr) } {
            return None;
        }
        Some(
            (0..unsafe { ffi::mlirDenseArrayGetNumElements(attr) })
                .map(|index| unsafe { ffi::mlirDenseI64ArrayGetElement(attr, index) })
                .collect(),
        )
    }

    pub fn f64_array_attribute<const N: usize>(self, name: &str) -> Option<[f64; N]> {
        // Check the element type and length before reading any values.
        let attr = unsafe { ffi::mlirOperationGetAttributeByName(self.raw, string_ref(name)) };
        if attr.ptr.is_null()
            || !unsafe { ffi::mlirAttributeIsADenseF64Array(attr) }
            || unsafe { ffi::mlirDenseArrayGetNumElements(attr) } != N as isize
        {
            return None;
        }
        Some(std::array::from_fn(|index| unsafe {
            ffi::mlirDenseF64ArrayGetElement(attr, index as isize)
        }))
    }

    pub fn operand_count(self) -> usize {
        unsafe { ffi::mlirOperationGetNumOperands(self.raw) as usize }
    }

    pub fn operand(self, index: usize) -> Option<ValueRef<'m>> {
        (index < self.operand_count()).then(|| ValueRef {
            raw: unsafe { ffi::mlirOperationGetOperand(self.raw, index as isize) },
            owner: PhantomData,
        })
    }

    pub fn result_count(self) -> usize {
        unsafe { ffi::mlirOperationGetNumResults(self.raw) as usize }
    }

    pub fn result(self, index: usize) -> Option<ValueRef<'m>> {
        (index < self.result_count()).then(|| ValueRef {
            raw: unsafe { ffi::mlirOperationGetResult(self.raw, index as isize) },
            owner: PhantomData,
        })
    }

    pub fn regions(self) -> impl Iterator<Item = RegionRef<'m>> {
        let count = unsafe { ffi::mlirOperationGetNumRegions(self.raw) };
        (0..count).map(move |index| RegionRef {
            raw: unsafe { ffi::mlirOperationGetRegion(self.raw, index) },
            owner: PhantomData,
        })
    }

    /// Visit immediate children in region, block and operation order. Use
    /// `regions()` when the distinction between regions matters to the caller.
    pub fn children(self) -> impl Iterator<Item = OperationRef<'m>> {
        self.regions()
            .flat_map(RegionRef::blocks)
            .flat_map(BlockRef::operations)
    }

    pub fn span(self) -> Span {
        // The location belongs to the context. Only access its coordinates for this kind.
        unsafe {
            let location = ffi::mlirOperationGetLocation(self.raw);
            if ffi::mlirLocationIsAFileLineColRange(location) {
                return Span {
                    line: ffi::mlirLocationFileLineColRangeGetStartLine(location) as usize,
                    column: ffi::mlirLocationFileLineColRangeGetStartColumn(location) as usize,
                    source: 0,
                };
            }
        }
        Span {
            line: 1,
            column: 1,
            source: 0,
        }
    }

    pub fn source_file(self) -> Option<&'m str> {
        // File locations and their names are immutable and owned by the module context.
        let name = unsafe {
            let location = ffi::mlirOperationGetLocation(self.raw);
            if !ffi::mlirLocationIsAFileLineColRange(location) {
                return None;
            }
            ffi::mlirIdentifierStr(ffi::mlirLocationFileLineColRangeGetFilename(location))
        };
        std::str::from_utf8(unsafe { bytes(name) }).ok()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ValueRef<'m> {
    raw: ffi::MlirValue,
    owner: PhantomData<&'m Module>,
}

impl PartialEq for ValueRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.raw.ptr == other.raw.ptr
    }
}
impl Eq for ValueRef<'_> {}
impl Hash for ValueRef<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.ptr.hash(state);
    }
}

impl<'m> ValueRef<'m> {
    pub(crate) fn uses(self) -> impl Iterator<Item = (OperationRef<'m>, usize)> {
        // Uses are borrowed from the module. Its operation graph cannot be mutated
        // while these handles are available.
        let first = unsafe { ffi::mlirValueGetFirstUse(self.raw) };
        std::iter::successors((!first.ptr.is_null()).then_some(first), |usage| {
            let next = unsafe { ffi::mlirOpOperandGetNextUse(*usage) };
            (!next.ptr.is_null()).then_some(next)
        })
        .map(|usage| unsafe {
            let owner = ffi::mlirOpOperandGetOwner(usage);
            let index = ffi::mlirOpOperandGetOperandNumber(usage) as usize;
            (
                OperationRef::from_raw(owner).expect("use has an owner"),
                index,
            )
        })
    }

    pub fn defining_operation(self) -> Option<OperationRef<'m>> {
        // Block arguments are values too, but have no defining operation.
        if !unsafe { ffi::mlirValueIsAOpResult(self.raw) } {
            return None;
        }
        OperationRef::from_raw(unsafe { ffi::mlirOpResultGetOwner(self.raw) })
    }

    pub fn type_name(self) -> String {
        let mut output = Vec::new();
        // The type and callback buffer remain live until printing returns.
        unsafe {
            ffi::mlirTypePrint(
                ffi::mlirValueGetType(self.raw),
                Some(append_bytes),
                data(&mut output),
            )
        };
        String::from_utf8_lossy(&output).into_owned()
    }

    pub fn xr_type(self) -> Result<Type, String> {
        let name = self.type_name();
        name.strip_prefix("!xr.")
            .and_then(Type::parse)
            .ok_or_else(|| format!("XR value type {name}"))
    }
}

#[derive(Clone, Copy)]
pub struct RegionRef<'m> {
    raw: ffi::MlirRegion,
    owner: PhantomData<&'m Module>,
}

impl<'m> RegionRef<'m> {
    pub fn blocks(self) -> impl Iterator<Item = BlockRef<'m>> {
        let first = BlockRef::from_raw(unsafe { ffi::mlirRegionGetFirstBlock(self.raw) });
        std::iter::successors(first, |block| {
            BlockRef::from_raw(unsafe { ffi::mlirBlockGetNextInRegion(block.raw) })
        })
    }
}

#[derive(Clone, Copy)]
pub struct BlockRef<'m> {
    raw: ffi::MlirBlock,
    owner: PhantomData<&'m Module>,
}

impl<'m> BlockRef<'m> {
    fn from_raw(raw: ffi::MlirBlock) -> Option<Self> {
        (!raw.ptr.is_null()).then_some(Self {
            raw,
            owner: PhantomData,
        })
    }

    pub fn operations(self) -> impl Iterator<Item = OperationRef<'m>> {
        let first = OperationRef::from_raw(unsafe { ffi::mlirBlockGetFirstOperation(self.raw) });
        std::iter::successors(first, |op| {
            OperationRef::from_raw(unsafe { ffi::mlirOperationGetNextInBlock(op.raw) })
        })
    }
}
