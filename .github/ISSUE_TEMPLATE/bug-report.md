---
name: Bug Report
about: Report a Bug
title: ''
labels: 'bug'
assignees: ''

---

### Affected Version

<!-- get the version you're running via

paru -V

-->

```sh
**Include your version!**
```

### Checklist

- [ ] I have checked the open and already closed [issues](https://github.com/Morganamilo/paru/issues) for duplicates

### Description



### Output


```console
**Include the FULL output of any relevant commands/configs**

**Don't cut parts of the input always include the FULL thing**
```

### Config-Files
#### Paru.conf

<!-- create the minimised dump of the used config file in the console via 

{
if [ -n "$PARU_CONF" ] && [ -f "$PARU_CONF" ]; then
  echo '=> paru uses $PARU_CONF config file: ' "$PARU_CONF"
  cat "$PARU_CONF";
elif [ -n "$XDG_CONFIG_HOME" ] && [ -f "$XDG_CONFIG_HOME/paru/paru.conf" ]; then
  echo '=> paru uses $XDG_CONFIG_HOME config file: ' "$XDG_CONFIG_HOME/paru/paru.conf"
  cat "$XDG_CONFIG_HOME/paru/paru.conf"
elif [ -n "$HOME" ] && [ -f "$HOME/.config/paru/paru.conf" ]; then
  echo '=> paru uses $XDG_CONFIG_HOME config file: ' "$XDG_CONFIG_HOME/paru/paru.conf"
  cat "$XDG_CONFIG_HOME/paru/paru.conf"
else
  echo '=> paru uses system-config file: /etc/paru.conf'
  cat /etc/paru.conf
fi;
} | grep -v -E "^#|^$"

-->

```sh
**Include your paru.conf!**
```

#### Pacman.conf

<!-- create the minimised dump of the used config file in the console via 

cat /etc/pacman.conf | grep -v -E "^#|^$"

-->

```sh
**Include your pacman.conf!**
```
