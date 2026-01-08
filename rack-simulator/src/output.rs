pub struct Output {
    verbose: bool,
}

impl Output {
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }

    pub fn step(&self, msg: &str) {
        if self.verbose {
            println!("==> {}", msg);
        }
    }

    pub fn detail(&self, label: &str, value: &str) {
        if self.verbose {
            println!("    {}: {}", label, value);
        }
    }

    pub fn success(&self, msg: &str) {
        println!("[OK] {}", msg);
    }

    pub fn error(&self, msg: &str) {
        eprintln!("[ERROR] {}", msg);
    }

    pub fn info(&self, msg: &str) {
        if self.verbose {
            println!("    {}", msg);
        }
    }

    #[allow(dead_code)]
    pub fn data(&self, label: &str, data: &[u8]) {
        if self.verbose {
            if data.len() <= 64 {
                println!("    {}: {:?}", label, data);
            } else {
                println!("    {}: [{} bytes]", label, data.len());
            }
        }
    }

    pub fn is_verbose(&self) -> bool {
        self.verbose
    }
}
