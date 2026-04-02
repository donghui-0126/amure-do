"""
amuredo Julia File-Based Backend Server

Implements the amuredo file-based backend protocol:
  - Watches `_cmd.txt` for a code string to execute
  - Writes captured output to `_out.txt`
  - Creates `_ready` on startup to signal readiness
  - Logs activity to `_server.log`

amuredo writes the code to `_cmd.txt`, this server executes it,
then writes results to `_out.txt` and deletes `_cmd.txt`.
"""

const CMD_FILE   = "_cmd.txt"
const OUT_FILE   = "_out.txt"
const READY_FILE = "_ready"
const LOG_FILE   = "_server.log"
const POLL_SECS  = 0.25

function log(msg::String)
    ts = string(now())
    open(LOG_FILE, "a") do f
        println(f, "[$ts] $msg")
    end
    println("[$ts] $msg")
    flush(stdout)
end

function execute_code(code::String)::String
    buf = IOBuffer()
    try
        # Redirect stdout into buf for the duration of eval
        redirect_stdout(buf) do
            include_string(Main, code)
        end
    catch e
        print(buf, sprint(showerror, e, catch_backtrace()))
    end
    return String(take!(buf))
end

function main()
    using Dates

    log("amuredo Julia backend starting")

    # Signal readiness
    open(READY_FILE, "w") do f
        write(f, "ready\n")
    end
    log("Ready. Watching for $CMD_FILE ...")

    while true
        sleep(POLL_SECS)

        isfile(CMD_FILE) || continue

        code = try
            read(CMD_FILE, String)
        catch
            continue
        end

        log("Received command ($(length(code)) chars), executing ...")

        output = execute_code(code)

        open(OUT_FILE, "w") do f
            write(f, output)
        end

        rm(CMD_FILE; force=true)
        log("Done. Output written to $OUT_FILE")
    end
end

main()
