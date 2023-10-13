# crab_trap
A convenient lightweight reverse shell manager/c2.  
crab_trap allows to to capture multiple reverse shells in a single window.  
What's more? crab_trap allows you to easily make your pty sessions interactive, no more `stty raw -echo;fg` shenanigans.

other cool features:
- shell verification: crab_trap automatically verifies that a tcp connection is a shell before it adds it to your list
- shell aliasing, easily rename your shells
- ignores CTRL+c in non-interactive mode to stop you from accidentally closing your shell


  
This project is still under active development, PRs are welcome 

## TODO:
- Add testing
- Add support for multiple running instances (maybe a client/server architecture)
- Notifications when new shell is caught