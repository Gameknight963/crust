use std::ffi::OsString;

#[allow(dead_code)] // The project driver only uses argument encoding.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Build,
    BuildDependency,
    EmitC,
    EmitHeader,
    Check,
    Tokens,
}

// The driver sends encoded OS arguments on stdin to avoid command-line length
// limits and preserve paths that are not Unicode.
#[allow(dead_code)] // Used by the project driver.
pub fn encode(arguments: &[OsString]) -> Vec<Vec<u32>> {
    return arguments
        .iter()
        .map(|argument| {
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                return argument.encode_wide().map(u32::from).collect();
            }
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                return argument.as_bytes().iter().copied().map(u32::from).collect();
            }
        })
        .collect();
}

#[allow(dead_code)] // Used by the compiler.
pub fn decode(arguments: Vec<Vec<u32>>) -> Result<Vec<OsString>, String> {
    return arguments
        .into_iter()
        .map(|argument| {
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStringExt;
                let units: Vec<u16> = argument
                    .into_iter()
                    .map(u16::try_from)
                    .collect::<Result<_, _>>()
                    .map_err(|_| "invalid OS argument")?;
                return Ok(OsString::from_wide(&units));
            }
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStringExt;
                let units: Vec<u8> = argument
                    .into_iter()
                    .map(u8::try_from)
                    .collect::<Result<_, _>>()
                    .map_err(|_| "invalid OS argument")?;
                return Ok(OsString::from_vec(units));
            }
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_arguments_round_trip_os_paths_and_large_requests() {
        let mut arguments = vec![OsString::from("space \"quote\" \u{2603}")];
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            arguments.push(OsString::from_wide(&[0xd800, 0x61]));
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            arguments.push(OsString::from_vec(vec![0xff, 0x61]));
        }
        arguments.extend((0..1000).map(|index| {
            OsString::from(format!(
                "/long project directory/protected header {index}.h"
            ))
        }));
        let request = serde_json::to_vec(&encode(&arguments)).unwrap();
        assert!(request.len() > 32768);
        assert_eq!(
            decode(serde_json::from_slice(&request).unwrap()).unwrap(),
            arguments
        );
        assert!(decode(vec![vec![u32::MAX]]).is_err());
    }
}
