/// Convert Processor Version strings into more meaningful CPU models
/// Example: Intel(R) Xeon(R) CPU E3-1240 v3 @ 3.40GHz -> IntelXeonE3-1240v3
/// Example: AMD Ryzen 7 1700X Eight-Core Processor -> AMDRyzen71700X
/// Example: AMD EPYC 7C13 64-Core Processor -> AMDEPYC7C13
pub fn platform_name_processor_version(cpu_model: &str) -> String {
    if processor_version_is_amd(cpu_model) {
        processor_version_simplify_amd(cpu_model)
    } else if processor_version_is_intel(cpu_model) {
        processor_version_simplify_intel(cpu_model)
    } else {
        processor_version_simplify_generic(cpu_model)
    }
}

fn processor_version_is_intel(model: &str) -> bool {
    model.to_lowercase().starts_with("intel")
}

fn processor_version_is_amd(model: &str) -> bool {
    model.to_lowercase().starts_with("amd")
}

fn processor_version_simplify_intel(model: &str) -> String {
    let parts: Vec<_> = model
        .split(" ")
        .map(|word| word.replace("(R)", "").replace("-", ""))
        .take_while(|word| word != "@")
        .filter(|word| word != "CPU")
        .collect();

    parts.join("")
}

fn processor_version_simplify_amd(model: &str) -> String {
    let parts: Vec<_> = model
        .split(" ")
        .map(|word| word.replace("-", ""))
        .take_while(|word| !word.to_lowercase().ends_with("core"))
        .collect();

    parts.join("")
}

fn processor_version_simplify_generic(model: &str) -> String {
    model.replace(" ", "").replace("-", "")
}

#[cfg(test)]
mod tests {
    use super::platform_name_processor_version;

    #[test]
    fn prcessor_version_to_platform_name() {
        let cases = vec![
            (
                "Intel(R) Xeon(R) CPU E3-1240 v3 @ 3.40GHz",
                "IntelXeonE31240v3",
            ),
            ("AMD Ryzen 7 1700X Eight-Core Processor", "AMDRyzen71700X"),
            ("AMD EPYC 7C13 64-Core Processor", "AMDEPYC7C13"),
            ("Unknown MODEL 1234", "UnknownMODEL1234"),
        ];

        for (testcase, expected) in cases {
            assert_eq!(&platform_name_processor_version(testcase), expected);
        }
    }
}
