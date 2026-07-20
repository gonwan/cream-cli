### Cream CLI
CLI to collect and generate DLC list for CreamAPI config file

My toy project to work with rust. Supports Windows/Linux/MacOS. Supports Proton/Crossover environments.

#### Commandline Options
```shell
$ cream-cli -h
CLI to collect and generate DLC list for CreamAPI config file

Usage: cream-cli.exe [OPTIONS] --appid <APPID> --output <OUTPUT>

Options:
      --appid <APPID>    Steam appid
      --output <OUTPUT>  Steam game directory
      --proton           Whether it is a proton or crossover environment
      --api <API>        Select steam api to use (debugging) [default: 1]
  -h, --help             Print help
  -V, --version          Print version
```
```shell
# windows
$ cream-cli --appid 1158310 --output "D:\Program Files (x86)\Steam\steamapps\common\Crusader Kings III"
# linux (proton)
$ cream-cli --appid 1069660 --output "/home/gonwan/.local/share/Steam/steamapps/common/Age of Wonders 4" --proton
```

#### Limitations
- Steam APIs has rate limits.
- Public restful steam APIs may return incomplete DLC list. DLCs for limited time may be missing. But it works perfect 99% of the time.
- The [Python](https://github.com/valvepython/steam) and [.NET](https://github.com/SteamRE/SteamKit) version of Steam API uses protobuf approach. They does return full list of DLCs.
- There is also an [online version](https://www.steamcmd.net/).
