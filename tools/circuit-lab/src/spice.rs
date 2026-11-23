//! Synchronous libngspice adapter. Each solve is serialized; no background thread or callbacks retain pointers.
use crate::Result;
use std::{
    collections::BTreeMap,
    ffi::{c_char, c_int, c_void, CStr, CString},
    sync::{Mutex, OnceLock},
};
static SESSION: Mutex<()> = Mutex::new(());
static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());
static INITIALIZED: OnceLock<bool> = OnceLock::new();
#[repr(C)]
struct Vector {
    name: *mut c_char,
    kind: c_int,
    flags: i16,
    real: *mut f64,
    complex: *mut c_void,
    length: c_int,
}
#[link(name = "ngspice")]
extern "C" {
    fn ngSpice_Init(
        out: Option<extern "C" fn(*mut c_char, c_int, *mut c_void) -> c_int>,
        status: Option<extern "C" fn(*mut c_char, c_int, *mut c_void) -> c_int>,
        exit: Option<extern "C" fn(c_int, bool, bool, c_int, *mut c_void) -> c_int>,
        data: Option<extern "C" fn(*mut c_void, c_int, *mut c_void) -> c_int>,
        init: Option<extern "C" fn(*mut c_void, c_int, *mut c_void) -> c_int>,
        background: Option<extern "C" fn(bool, c_int, *mut c_void) -> c_int>,
        user: *mut c_void,
    ) -> c_int;
    fn ngSpice_Circ(lines: *mut *mut c_char) -> c_int;
    fn ngSpice_Command(command: *mut c_char) -> c_int;
    fn ngGet_Vec_Info(name: *mut c_char) -> *mut Vector;
}
extern "C" fn output(line: *mut c_char, _: c_int, _: *mut c_void) -> c_int {
    if !line.is_null() {
        // The synchronous callback owns this text only until return; copy immediately.
        if let Ok(mut log) = LOG.lock() {
            log.push(unsafe { CStr::from_ptr(line) }.to_string_lossy().into());
        }
    }
    0
}
extern "C" fn exited(code: c_int, _: bool, _: bool, _: c_int, _: *mut c_void) -> c_int {
    if let Ok(mut log) = LOG.lock() {
        log.push(format!("NGSPICE_EXIT {code}"));
    }
    0
}
fn command(command: &str) -> Result<()> {
    let command = CString::new(command)?;
    // ngspice consumes the mutable C string synchronously.
    if unsafe { ngSpice_Command(command.as_ptr().cast_mut()) } != 0 {
        return Err("ngspice command failed".into());
    }
    Ok(())
}
pub struct Trace {
    pub vectors: BTreeMap<String, Vec<f64>>,
    pub log: String,
}
impl Trace {
    pub fn last(&self, name: &str) -> Result<f64> {
        self.vectors
            .get(name)
            .and_then(|v| v.last())
            .copied()
            .ok_or_else(|| format!("missing {name}").into())
    }
    pub fn csv(&self) -> Result<String> {
        let n = self.vectors.values().next().ok_or("empty trace")?.len();
        if self.vectors.values().any(|v| v.len() != n) {
            return Err("inconsistent trace lengths".into());
        }
        let mut s = self.vectors.keys().cloned().collect::<Vec<_>>().join(",");
        s.push('\n');
        for i in 0..n {
            s.push_str(
                &self
                    .vectors
                    .values()
                    .map(|v| format!("{:.10e}", v[i]))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            s.push('\n');
        }
        Ok(s)
    }
}
pub fn solve(circuit: &str, analysis: &str, names: &[&str]) -> Result<Trace> {
    let _guard = SESSION.lock().map_err(|_| "ngspice session poisoned")?;
    // The library has process-global state. Initialize exactly once and never use bg_run.
    if !INITIALIZED.get_or_init(|| unsafe {
        ngSpice_Init(
            Some(output),
            None,
            Some(exited),
            None,
            None,
            None,
            std::ptr::null_mut(),
        ) == 0
    }) {
        return Err("ngspice initialization failed".into());
    }
    command("destroy all")?;
    LOG.lock().unwrap().clear();
    command("version")?;
    let lines: Vec<CString> = circuit
        .lines()
        .map(CString::new)
        .collect::<std::result::Result<_, _>>()?;
    let mut pointers: Vec<_> = lines.iter().map(|s| s.as_ptr().cast_mut()).collect();
    pointers.push(std::ptr::null_mut());
    // Strings and terminating pointer remain alive throughout load.
    if unsafe { ngSpice_Circ(pointers.as_mut_ptr()) } != 0 {
        return Err("ngspice circuit load failed".into());
    }
    command(analysis)?;
    let log = LOG.lock().unwrap().join("\n");
    if log.contains("NGSPICE_EXIT")
        || log.to_lowercase().contains("error")
        || log.contains("aborted")
    {
        return Err(log.into());
    }
    let mut vectors = BTreeMap::new();
    for name in names {
        let c = CString::new(*name)?;
        // Copy all borrowed vectors before another command can invalidate them.
        let values = unsafe {
            let p = ngGet_Vec_Info(c.as_ptr().cast_mut());
            if p.is_null() || (*p).length <= 0 || (*p).real.is_null() {
                return Err(format!("missing real vector {name}: {log}").into());
            }
            std::slice::from_raw_parts((*p).real, (*p).length as usize).to_vec()
        };
        if values.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite circuit result".into());
        }
        vectors.insert((*name).into(), values);
    }
    Ok(Trace { vectors, log })
}
