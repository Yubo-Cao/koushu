#!/usr/bin/env python3
"""通过 /dev/uinput 造一个虚拟键盘，按住再松开 Ctrl+Alt+Space。

用来自动验证 evdev push-to-talk 路径，不需要人真的去按键。
"""
import ctypes, fcntl, os, struct, sys, time

UINPUT_MAX_NAME_SIZE = 80
EV_SYN, EV_KEY = 0x00, 0x01
SYN_REPORT = 0
KEY_LEFTCTRL, KEY_LEFTALT, KEY_SPACE = 29, 56, 57

UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565
UI_DEV_CREATE = 0x5501
UI_DEV_DESTROY = 0x5502


class InputId(ctypes.Structure):
    _fields_ = [("bustype", ctypes.c_uint16), ("vendor", ctypes.c_uint16),
                ("product", ctypes.c_uint16), ("version", ctypes.c_uint16)]


class UinputUserDev(ctypes.Structure):
    _fields_ = [("name", ctypes.c_char * UINPUT_MAX_NAME_SIZE),
                ("id", InputId),
                ("ff_effects_max", ctypes.c_uint32),
                ("absmax", ctypes.c_int32 * 64),
                ("absmin", ctypes.c_int32 * 64),
                ("absfuzz", ctypes.c_int32 * 64),
                ("absflat", ctypes.c_int32 * 64)]


def emit(fd, etype, code, value):
    # struct input_event: timeval(16) + type(2) + code(2) + value(4)
    os.write(fd, struct.pack("@llHHi", 0, 0, etype, code, value))


def syn(fd):
    emit(fd, EV_SYN, SYN_REPORT, 0)


def main():
    hold = float(sys.argv[1]) if len(sys.argv) > 1 else 1.5
    delay = float(sys.argv[2]) if len(sys.argv) > 2 else 0.0
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
    for key in (KEY_LEFTCTRL, KEY_LEFTALT, KEY_SPACE):
        fcntl.ioctl(fd, UI_SET_KEYBIT, key)

    dev = UinputUserDev()
    dev.name = b"fun-asr-ptt-test-keyboard"
    dev.id = InputId(bustype=0x03, vendor=0x1234, product=0x5678, version=1)
    os.write(fd, bytes(dev))
    fcntl.ioctl(fd, UI_DEV_CREATE)
    time.sleep(1.0)  # let udev settle
    if delay:
        print(f"设备已创建，等 {delay}s 后再按键（让监听方先枚举到它）", flush=True)
        time.sleep(delay)

    print(f"虚拟键盘就绪，按下 Ctrl+Alt+Space，保持 {hold}s")
    for key in (KEY_LEFTCTRL, KEY_LEFTALT, KEY_SPACE):
        emit(fd, EV_KEY, key, 1)
    syn(fd)

    time.sleep(hold)

    print("松开")
    for key in (KEY_SPACE, KEY_LEFTALT, KEY_LEFTCTRL):
        emit(fd, EV_KEY, key, 0)
    syn(fd)

    time.sleep(0.4)
    fcntl.ioctl(fd, UI_DEV_DESTROY)
    os.close(fd)
    print("虚拟键盘已移除")


main()
