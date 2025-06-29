use anyhow::Result;
use console::{style, Term};
use std::io::ErrorKind;
use std::process::Command;
use std::time::Instant;

pub struct TaskRunner {
    term: Term,
    num_tasks: u32,
    current_task: u32,
    now: Instant,
    descr: String,
    verbose: bool,
    started: bool,
}

impl TaskRunner {
    pub fn new(num_tasks: u32, verbose: bool) -> Self {
        Self {
            term: Term::stdout(),
            num_tasks,
            current_task: 0,
            now: Instant::now(),
            descr: "".into(),
            verbose,
            started: false,
        }
    }

    fn task_id(&self) -> String {
        style(format!("[{}/{}]", self.current_task + 1, self.num_tasks))
            .force_styling(true)
            .to_string()
    }

    pub fn start_task(&mut self, descr: impl Into<String>) {
        if self.started {
            self.finish_task(true, true);
        }
        self.now = Instant::now();
        self.descr = descr.into();
        self.started = true;
        println!("{} {}", self.task_id(), &self.descr);
    }

    fn finish_task(&mut self, skipped: bool, clear_last: bool) {
        self.started = false;
        if clear_last {
            self.term.clear_last_lines(1).unwrap();
        }
        let status = if skipped {
            "[SKIPPED]".to_string()
        } else {
            let time = self.now.elapsed();
            format!("[{}ms]", time.as_millis())
        };
        println!("{} {} {}", self.task_id(), &self.descr, status,);
        self.current_task += 1;
    }

    pub fn end_task(&mut self) {
        self.finish_task(false, !self.verbose);
    }

    pub fn end_verbose_task(&mut self) {
        self.finish_task(false, false);
    }
}

pub fn run(mut command: Command, verbose: bool) -> Result<()> {
    fn print_error(command: &Command, status: Option<i32>) {
        let program = command.get_program().to_str().unwrap();
        let args = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect::<Vec<_>>()
            .join(" ");
        let status = if let Some(code) = status {
            format!(" exited with {}", code)
        } else {
            Default::default()
        };
        println!("{} {} {} {}", style("[ERROR]").red(), program, args, status);
    }

    let program = command.get_program().to_string_lossy().to_string();
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");

    let result = if !verbose {
        command.output()
    } else {
        command.status().map(|status| {
            use std::process::Output;
            // Fake Output for compatibility
            Output {
                status,
                stdout: vec![],
                stderr: vec![],
            }
        })
    };

    match result {
        Ok(output) => {
            if !output.status.success() {
                print_error(&command, output.status.code());
                if !verbose {
                    let stdout = std::str::from_utf8(&output.stdout)?;
                    print!("{}", stdout);
                    let stderr = std::str::from_utf8(&output.stderr)?;
                    print!("{}", stderr);
                }
                std::process::exit(1);
            }
        }
        Err(e) => {
            if e.kind() == ErrorKind::NotFound {
                eprintln!(
                    "{} Command not found: {} {}",
                    style("[ERROR]").red(),
                    program,
                    args
                );
            } else {
                eprintln!(
                    "{} Failed to execute '{} {}': {}",
                    style("[ERROR]").red(),
                    program,
                    args,
                    e
                );
            }
            std::process::exit(1);
        }
    }
    Ok(())
}
