//! File access in the folders an agent can write (the user's skills,
//! plugins, design systems, memory under `<data>`): read without following
//! a link or waiting on a FIFO, write through a new file of a random name.
//! Shared by `od-content` and `od-store`.

pub mod nofollow;
