# Crab Hole
## SSH-made-convenient

Have you ever wanted to remote into a computer, only to find that port-forwarding
is turned off?

Utah Tech University exists on a heavily managed network, and there are two ways
out of the network:
1) Physically leave the school
2) Tunnel through the CS Department's SSH Server

Option 1 is often not feasible, especially for on-campus students.
Option 2 is still heavily limited; an account must be made per-student, with username
and password. The actual outgoing connections are limited to a single port.

Crab Hole provides a solution:

By generating a single-use magic password, a relay server can bypass port forwarding
and provide an interactive session on a remote machine.

Each magic password is unqiue and generally typeable, meant to be shared verbally or
over text message.

The only requirement is that both machines have a copy of the application to run!

For example:

M (remote machine) is controlled by Jarod Whiting.
L (local machine)  is controlled by Me.

I need access to Jarod's Computer, M. He starts up Crab Hole, and tells me the password:
```
1-noisy-llama
```
I type in that password to my client, and I'm in!

A benefit is that any other connected clients can look at and contribute to the same session.
If a third machine also put in the magic code, it could see each command I send in addition
to each response from the server.
