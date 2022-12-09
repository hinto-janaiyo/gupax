// Gupax - GUI Uniting P2Pool And XMRig
//
// Copyright (c) 2022 hinto-janaiyo
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

// Handling of [sudo] for XMRig.
// [zeroize] is used to wipe the memory after use.
// Only gets imported in [main.rs] for Unix.

use zeroize::Zeroize;
use std::{
	thread,
	sync::{Arc,Mutex},
	process::*,
	io::Write,
	path::PathBuf,
};
use crate::{
	Helper,
	disk::Xmrig,
	ProcessSignal,
	constants::*,
};
use log::*;

#[derive(Debug,Clone)]
pub struct SudoState {
	pub testing: bool, // Are we attempting a sudo test right now?
	pub success: bool, // Was the sudo test a success?
	pub hide: bool, // Are we hiding the password?
	pub msg: String, // The message shown to the user if unsuccessful
	pub pass: String, // The actual password wrapped in a [SecretVec]
	pub signal: ProcessSignal, // Main GUI will set this depending on if we want [Start] or [Restart]
}

impl SudoState {
	pub fn new() -> Self {
		Self {
			testing: false,
			success: false,
			hide: true,
			msg: "".to_string(),
			pass: String::with_capacity(256),
			signal: ProcessSignal::None,
		}
	}

	// Resets the state.
	pub fn reset(state: &Arc<Mutex<Self>>) {
		Self::wipe(&state);
		let mut state = state.lock().unwrap();
		state.testing = false;
		state.success = false;
		state.signal = ProcessSignal::None;
	}

	// Swaps the pass with another 256-capacity String,
	// zeroizes the old and drops it.
	pub fn wipe(state: &Arc<Mutex<Self>>) {
		let mut new = String::with_capacity(256);
		// new is now == old, and vice-versa.
		std::mem::swap(&mut new, &mut state.lock().unwrap().pass);
		// we're wiping & dropping the old pass here.
		new.zeroize();
		std::mem::drop(new);
		info!("Sudo | Password wipe with 0's ... OK");
	}

	// Spawns a thread and tests sudo with the provided password.
	// Sudo takes the password through STDIN via [--stdin].
	// Sets the appropriate state fields on success/failure.
	pub fn test_sudo(state: Arc<Mutex<Self>>, helper: &Arc<Mutex<Helper>>, xmrig: &Xmrig, path: &PathBuf) {
		let helper = Arc::clone(helper);
		let xmrig = xmrig.clone();
		let path = path.clone();
		thread::spawn(move || {
			// Set to testing
			state.lock().unwrap().testing = true;

			// Make sure sudo timestamp is reset
			let reset = Command::new("sudo")
				.arg("--reset-timestamp")
				.stdout(Stdio::piped())
				.stderr(Stdio::piped())
				.stdin(Stdio::piped())
				.status();
			match reset {
				Ok(_)  => info!("Sudo | Resetting timestamp ... OK"),
				Err(e) => {
					error!("Sudo | Couldn't reset timestamp: {}", e);
					Self::wipe(&state);
					state.lock().unwrap().msg = format!("Sudo error: {}", e);
					state.lock().unwrap().testing = false;
					return
				},
			}

			// Spawn testing sudo
			let mut sudo = Command::new("sudo")
				.args(["--stdin", "--validate"])
				.stdout(Stdio::piped())
				.stderr(Stdio::piped())
				.stdin(Stdio::piped())
				.spawn()
				.unwrap();

			// Write pass to STDIN
			let mut stdin = sudo.stdin.take().unwrap();
			stdin.write_all(state.lock().unwrap().pass.as_bytes()).unwrap();
			drop(stdin);

			// Sudo re-prompts and will hang.
			// To workaround this, try checking
			// results for 5 seconds in a loop.
			for i in 1..=5 {
				match sudo.try_wait() {
					Ok(Some(code)) => if code.success() {
						info!("Sudo | Password ... OK!");
						state.lock().unwrap().success = true;
						break
					},
					Ok(None) => {
						info!("Sudo | Waiting [{}/5]...", i);
						std::thread::sleep(SECOND);
					},
					Err(e) => {
						error!("Sudo | Couldn't reset timestamp: {}", e);
						Self::wipe(&state);
						state.lock().unwrap().msg = format!("Sudo error: {}", e);
						state.lock().unwrap().testing = false;
						return
					},
				}
			}
			sudo.kill();
			if state.lock().unwrap().success {
				match state.lock().unwrap().signal {
					ProcessSignal::Restart => crate::helper::Helper::restart_xmrig(&helper, &xmrig, &path, Arc::clone(&state)),
					_ => crate::helper::Helper::start_xmrig(&helper, &xmrig, &path, Arc::clone(&state)),
				}
			} else {
				state.lock().unwrap().msg = "Incorrect password!".to_string();
				Self::wipe(&state);
			}
		});
	}
}