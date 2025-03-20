"""
Experimental module that extends the multiprocessing.Process class to redirect
stdout and stderr to pipes that can be read line-by-line in real time.

"""

import multiprocessing
import os
import sys
import time


class ProcessExtended(multiprocessing.Process):
    def __init__(self, target, args=(), kwargs=None):
        if kwargs is None:
            kwargs = {}
        # Store the target function and its arguments.
        self._target_func = target
        self._target_args = args
        self._target_kwargs = kwargs
        # Create OS pipes for stdout and stderr.
        self._stdout_pipe_read, self._stdout_pipe_write = os.pipe()
        self._stderr_pipe_read, self._stderr_pipe_write = os.pipe()
        super().__init__()

    def run(self):
        """
        In the child process, close the parent's copy of the read ends,
        duplicate the write ends to stdout and stderr, and execute the target function.
        """
        # In child: close the parent's read ends.
        os.close(self._stdout_pipe_read)
        os.close(self._stderr_pipe_read)

        # Redirect stdout and stderr to the write ends of the pipes.
        os.dup2(self._stdout_pipe_write, sys.stdout.fileno())
        os.dup2(self._stderr_pipe_write, sys.stderr.fileno())

        # Reassign the Python-level streams with line buffering.
        sys.stdout = os.fdopen(sys.stdout.fileno(), "w", buffering=1)
        sys.stderr = os.fdopen(sys.stderr.fileno(), "w", buffering=1)

        try:
            self._target_func(*self._target_args, **self._target_kwargs)
        finally:
            # Flush and close the write ends so the parent can detect EOF.
            sys.stdout.flush()
            sys.stderr.flush()
            os.close(self._stdout_pipe_write)
            os.close(self._stderr_pipe_write)

    def start(self):
        """
        Start the child process and, in the parent process, close the write ends
        of the pipes so that reading from the read ends will end once the child closes them.
        """
        super().start()
        # In the parent process, close the write ends.
        os.close(self._stdout_pipe_write)
        os.close(self._stderr_pipe_write)

    @property
    def stdout(self):
        """
        Returns a file object that can be iterated line-by-line to read the child's stdout.
        """
        if not hasattr(self, "_stdout_file"):
            self._stdout_file = os.fdopen(self._stdout_pipe_read, "r", buffering=1)
        return self._stdout_file

    @property
    def stderr(self):
        """
        Returns a file object that can be iterated line-by-line to read the child's stderr.
        """
        if not hasattr(self, "_stderr_file"):
            self._stderr_file = os.fdopen(self._stderr_pipe_read, "r", buffering=1)
        return self._stderr_file


# -------------------------------
# Example usage:
# -------------------------------
def worker():
    for i in range(5):
        print(f"stdout line {i}")
        print(f"stderr line {i}", file=sys.stderr)
        time.sleep(1)


if __name__ == "__main__":
    proc = ProcessExtended(target=worker)
    proc.start()

    # Iterate over stdout in real time.
    # Note: In a real-world scenario you might want to use threads or select/poll to
    # concurrently process stdout and stderr if both are needed in real time.
    try:
        for line in proc.stdout:
            print("Streamed stdout:", line, end="")
    except KeyboardInterrupt:
        pass

    # Wait for the process to finish.
    proc.join()

    # Optionally, process any remaining stderr output.
    for line in proc.stderr:
        print("Streamed stderr:", line, end="")
