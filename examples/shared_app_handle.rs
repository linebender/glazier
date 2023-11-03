use std::thread;

use glazier::Application;
use instant::Duration;

fn main() {
    let app = Application::new().expect("application failed to initialize");

    // Get a handle from the app
    let handle = app.get_handle().expect("failed to get app handle");

    // Spawn a new thread and move the app handle there
    thread::spawn(move || {
        // Take a quick break
        println!("Starting sleep");
        thread::sleep(Duration::from_secs(5));
        println!("Done sleeping");

        // Use the app handle to run code back on the main thread
        handle.run_on_main(|_| {
            println!("Running on main!");
        });
    });

    app.run(None);
}
