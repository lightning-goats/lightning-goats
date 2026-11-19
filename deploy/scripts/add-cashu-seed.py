#!/usr/bin/env python3
"""Offline, interactive preparation of a dedicated Cashu wallet's 64-byte seed.

Python 3 standard library only. No network, service activation or journal access.
Run on a trusted operator machine. The default is private staging outside Git;
installation into the isolated runtime is a separate deployment step.
"""
import argparse
import base64
import getpass
import hashlib
import hmac
import os
from pathlib import Path
import resource
import secrets
import stat
import sys
import unicodedata
import warnings
import zlib

# English list from the checksum-verified bip39 2.2.2 crate (CC0-1.0).
# Crate SHA256: 90dbd31c98227229239363921e60fcf5e558e43ec69094d46fc4996f08d1d5bc
# BIP39: https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki
WORDLIST_SHA256 = '2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda'
WORDLIST_B85 = 'c-k$S*_NX?5Cz};m)HiZE<lU~cDcWPA})RE_$G_cl1k+!)o>rSXWeeYy)5PULYEP!=OMnm?&Ic7uk9{5+VIHz*`AQIQ|9p)dzX}F3O^q8yy*4%RYt4gHBirflm{38ou&D-)qHIEqu7_nQJdJVT90#BZo_lfDe-*zE00`W@1K9D5V98WT*UXVJw|DfjIG{nahitGB-O4@ZLmM{P-t=<{n_Waja<LRfp=xS<%`nx+IUyG+*yXk^W}bukNT?gZW(>Ql##j?@0al_)(^N=JGj)%kN4LAHtM%PufQ?V#_}1yjAk8<@odt+Wt*Y(!tis2opfs(majE#kC%+v9wj~BX6xBz`?$@G>bJ7Wy6q>AHY$1KA`IQ?@yZAN6LO5(Hb~2Vr=@<&s6cG|_QBEZb*Zy=`#3a-weEY1nCz`kXM4Pa%r8R}th3G5DA(HR0NQH5+e&4PPWhzSed*}PS}X6|$v@j}$2dhq+EG&A_|*1u8@j)J^iA(WCcpPFm~y?JD;4$QwbGB{wTuV1N2M#rF+6@Gqa8Soa-2`!{S)x>pD><J<?5UxWSNEY2;0x23g=3p_t+vr@A?>?RjO!xhM!SvK?eI7d$l{C>4aIIA6`-HxAfb6xPO7iYh0+C3!1(UPwKo6GruJQeOKN#3nF9pVZGN1+4z%eqhIDXE*5H$?Ym61c(;Pi)PO(m=+0P~!uv?G;wGHT`F(s0E?lVHy^rGruxh-Xt#W)ZWJ!T{_i@SkhB93sRgYWh+{USR*991B-G|~1`HZ7AeXCWJ$M<1hiq}70<mn}G_tLy+2UV8q^ToLXnK5t;ZW+`v*w``-zE?rqm(;b?8thDgGy`1DfEAvd+N%=+QbT2Z-YY_Vuj?IP@d79H)%qjY42s0w)aF`+D6e<b*}WbL+5K8__xj6^`Vg&jvSnhB`KM_47KW-8#PP2;#WA)LDQLw>Xk~u+d75jb`@W)Wm*T~b3~hDG$JOTBwZ@?zUuBHvT^)ZvpAPV(aw<GgjSI4LmWB4u<u3dPOI~l!d;*8548?ClXPd7(86jWJUbZm&?AClu)cFnm{d;>1_q&S(%HSWdG!M0VhvfN>fMf$Ju(GHmJKzVXJ@GMy(EEovEpMt{ZtuaWwr`&8{~FlKk?;0nNVCn~Jw=#nV6NiMI3dkio0r{>;TKRgi}WM)^)uC3go^7deOT-m=bF!ttKjKWT?+|}GI==3j}cSL{4tKv^5d%ZevDg}6I`EpiYP3StJ^Q_5Xhu}>z>+P(V39i#a)I&+x4&qsTo_gix0mY$2>=vfDHHL>cm7p6dz5suapCD-9}4P%_yYiTO<r6@_3yO?Fw|W;<NE}ndNW6kgRsFG#B-w9ObWd>tH;}n>F=Vw5{MSv@(6AAHgei5r~2Z-;Qc=Ej*SOI><DMtV=MRS&B8)6mm|Mbag@e#q%kCtn~DoQ<-ALhhfW^`K!Z)GNYnpPbuo<QKxB?*zM|ER2Id<x2kGT3xJ|Tsx7t#CjAAdeXDDjQP*8#O4ZJ)^m(S5Z7P^X=p2WJi|wj^>qYg##nGy*Z||Zyd8mN^A7P9wv0HIdV${-r4Wjo`Plkj(^4XbDV#l2NdTPTVt%vjsFAT5Wiluz0ute8CWIDuE*z&*y(uVM{4WC8)OT`GUg+^^BWVDWv=~bn<&{~mAD6n1BUBC<UTHOhzmnbdft(}kJxY{`ei(PH!n$_LTEhW$N!-jE;EPdX2i#u@2jdS;RDfh?ON_C-~a7SS}|E;jIxi+aNOAFaRe?Crm5X<yLF+cIjW-k$+XAJW5a=M;FH1bn+JhSlg5P^Fl&retKbHv<^5=ISqXu2TddbjMT>nvH6B4}+_aaq~DNT~@e;mvfFnyfqy>vLSv(o)_jxN2G;CB?Ur9&GWQpq7kJ2N_dvuf61!1q12@AbL7s&#^N__gswmq?dV<{0O^>35taJ3B0tWL&1BpbQfk7s{Vi|QqO{mjdVQIr=p^GN}0W9iWS=N4RZ2ip*x&1M~Oo>b;z?U<BC7`Eb)0D72nT`2s}jSmn+Z3CDL46tSrt_>F}mYFB{Iq9u~r)Yii}B^}I7P<y~8z{zbl@H6aS*jPib>a-OIKVaF$eu0zO{S*#dNzuNLqz>d^YA%5yVKeM*L8R>&KI#!n}1Ns46QCGzCL|<CA*dT)UBkn>flH;rofF{w=0E+T?Dlz9Jr~0zbsJ&-q8b5TpJ-=ve+8YN&8l-PH`goGyCLkMdu;d9f09;`ffXh*XZWXV7)Md%yAL)TC7^i5&ah`8ue3-GsGZ~jVF4MR?bIt5Ed$5c%afmeRg%UMg7w<vNBj;ASz(p3&fT4isu|ZH-J{>iK<62{Q$8}$-Z})1j17+BRCh{MG(6|gn-5eMUt!hNvR|kWPIks(z1dQ9P5`ee{$L$qq8Mi;yzb&q?kw|NXR~v%~Q8cvg&4TI%m*REP{3`2&+Z|~{*v9|#F+gTjABdm5X?cf=%!u$csi&Pp#jMc5&xwG(;+P&G$MT5gbezTm3(wG$=DgMnvm$@ItP7Ge{)P{~Xg9g?o0uMQc^O=U<-gd=XQ+<N_&Y{NLffO{!D~>upylrT|CUr1w*2NnRj9|`T@3{p)94;{{IUdDHqe^7_IC9lAsR$pd`EwJw6|%{M}{yS0CoR`KD{YO3y8!{dXzx-@fuKqCu8ak+5A0xupJ8YWSlw;mmQb}t5NrW$}6N8%ub0lCjz}DTc&Y{shQ~5sVbXBB-+nL9}@Ed-Vu-cz!{jBq4G`{SK<^$XTnNOMUya<D*&Aes{@=<zuW{?;YcYbcQV3>O#tTWv082_`Njbi-Si1vnBsZo{Sl9SD!;dBA<&!>OMw<x9Q@)CA&z6o%ANGJrlsbs9v7G*Sz6O_62nairzZJa(OVOWl2*`?lWx>h9Z^koWHKxRs4UCND6{9rz+^JWS97Snpr>`Ugv}GKLh6baHzkcQ5oeLsnZu=)p){6}I)n>*L>Of`)u1o!5$>$7^^&H<NuciPgs;IP-Hz$p{9!jI+}wzpPWp6a(&r&TRD$i9Nd5OKiE}^OhZ!#yzgVpSx&NcaP!JqjN%Y)I{KC)Dhh-vhGya4(1a-5ji1~vS_z7uwXuJVA6@VG;rBg2lG#Az8ti71atVW+N0htR1XY#gDpN3;5m=+XGNDCqx^P>F&*S$;YJ&1P%I>e>sYvyt`oSqeoxjvr?*6d8rPVt=Jfd~n!H9$$)ERv2e&Dv+UA@lJK?t;BYlngqAriPmnD}!&{0L;5hHS-8FVX{E*0{XN*G6DFKyGZYv*_-)Pcv!+#bY^G-y*|gqfG^DS{9SLaLG6{Oi=b-)RxFAn;`UN`5pm&mzl<h*(tOeoGVrYo8+;{C@*>p({7n%fd|*K}y|k&~n`*R~FXZuc*+&vV1mxg4=4J;revOCasqnlg2cX?vYP%PVe7W9V=vpV{RquE4q3`i>B7jjSdZn*x{k;rWUUf^EYMI@8rsGv(VqP(YNYfQaN332ra$cUkR&Biqt6s|K>%{XEwX6^;U}X{sNKHz<oCTuAX9~1Dkqc+PhpAsP9ydL~ViJrQ8g5A|8zdIL!V5IWape4P6+)xPN*we;D%cfNm^1xlsQLp5*1|z*9$uMC=&5liMiM~G8&tkPF83S#I{u0!-V7J2JP5Xq5tL2qAz4gK7hQ!F@>1JFMnw{peCYF*?a!D(f6jcmUN!2;g4GNF7r5Y{DRRqn!!5}5p<$sUMiY8y;*f!iPX9vb<DvrZ>-t^N$YtqEld*k~-*_BLIzXlrJVyHcR($?ksj5GND+c0^EY2T9SHlV2#04GyXZ#=zf9jndEB230#UF%2q0%k-W0nbYQvVox|2dPTh7{QN#{@!>_&>NBWQ6PO$2AS?0LJl@es4b}0e+AUep~K0ueTX@8_N9uz-N9B*UFDW{xM(Z^{3o^HjLQMRwhxy1%7S|`i36^0O<{CP-k>8KfjogS`7E#6?~Yz+mGsf%gvFZ2cuq$`@~Jh8?!mw%xEk|fk4-o5MZP{Ap|*&G&2s9z<L9j8+~B)_cEfh5$@Y1;Gl+U8B4$3no?M<N85}pfpnnmDjB`lyiX3!6bdyEVga}os7`3pO+j3<LKc~_XeKTjznFtff@N_rflSe&JF>7lzzoFC#R=E+7;^$qZ@2{k3f6m6B`$*fm*2z)SUdMY5Sqn}4eSWKtaj5Ur{A8M<`@-Rc0VPC;pI8U?<U)#ceI?_5M<FV=dUYi`8GY=)$Jx$%SuPk1~dq6Nb>4-&uV;{*lZpCFx#s$yrSF(ANzE(6~kv&1(ft47@r$@rkoD)ir&8nT>$6Q*{-JBPa0$!<Eo8VdDV0Q@?=fQ(1qVhDm-%6aKqN&xJvw%6>n$N?ytxSlDe7}10n&dwr?Hv&K=_Tv>rmP0bGoPR`c-dm0p0jq;)bf4txcmJ$$4buTfBI@`0;<(rPS`7f0%4#{=GBcN{T`FG7MJ{Uby!)?)0qx*~zA@5Oi*G(uQ$I3D>rR>j1Gjk#!7LvE+R;g{dh8h*0~R`%`uhQMbLH32JYou68gHBgKO*DJn;T-?Q>V+hq=B`3Dl)YUeFUpC{AV07bp<;MR-h>+BTb&7Oke_&M#fdZP6TFI09^0ZR~0A)vz*_6w`_`pHq_$K$xg$z-xt!K6{*Dr~UNfL2izw~k&Ts)g@;x_E)GDvn2HYDW>rgPgBU2N{VZRT+|Ly8T}aOCrGPDXP37gTSA%ft~_l)UV|$uE0m+jt&wWlO-Zjh_zu@?Q0P&Foec?X4Qo5zrSE=Z1HXht0JDvG+9vj^QeP1YUA^Gs^Q;)uWo-PSd)ycdg$oDMyp6m5myF$}bSRNYe$$Lwh_NGc?1f_smPZzmvXW(*x%NW66>*8DpK5sQpH^*58;byHF<$fMG}d!R^rxs?k_=`I1L}+^P!Br0mT-9z+U;*DZNC2w&`ezZ;0xy<D&NxDa*E?9^-TlKpWjYLB>rS5^rmKx48AXY6T90vj?XZmZ3_#toM$nSx6o*?W(70ln^Y1t78d)qoMEZnyYddd&R1`u)y6jd0!SU&j)6XCJa^cb9XLbvq%-!)u~QgYZFQmiA~8FumA^J=lz=_aV8`-9*c-eC8VY+*N9OwAro_1Ikr08S6dDY=gR*cJ13fe6x1hL$(vl-<g{x^dWDsN^nKC^Baunp$}$o$G$n1F<EbZlM(}U3LzRjH*)q@hL4+C!DLVN*O&DO^L9cao|Z>OTP|z6m(kvo+23d_dY6DB)%LmJVhVOIyXEk(eVuWW5rl;oPjM4$ez`JtdpxV873p?*u_QINYtzJ8P<Zx=;|1_>)thIpU6$yu1IqDko^*HjcQ^CDSKQVVX%WICaQ|KOa?f|&dhmp+BsynTYUC%Wbw`tseRsn<O%QO|N%QvdwT)9=n=8)a90h?MR?*NOxxGGBI>A+GhhA^Yj^s8oLxM64X>139Dw$Q9OcRt`VV;|sfFw-}jaq=NH6PbHiTgE>W+Z7R>+!D`PK2I)`a^=%d0?+or3A+oVZU82|Bf%J%H88e?B}kCod$LL*{&TlE;53%WySANTf4~(SLM#WrkwuLQ=ALac9w>+i?>RftC0*u&)i^mn0L*O@E34MM7(N_=CK(HCHBp}3u})V21^C$Tms^2lF;DjrlxP&JP0D{24A{}Xqo;rr!D&k?aC#%$!rB5et%Ih*C~|jG#cmF_^2lgW+#|*x}gPRRAWLF|K{#QUS?;cjhQ!uB}S-?34S21`NnyHR7;rdyE#JJ#y?k4k|ko+M)MfKbj#U=HpWMTTn-EudKQqHer(S?#CYAJnc5gFt)v!T?0QF#tV*c4XEQ+tOo;L03>g!pa-lN|A!JGM&N54fx=B7detmYdtJcG<&NiOgHL~}`KAZ$QliXB$VcX$Bx%+doIg2DpJ`3gA)F+Q``!Tti1U^(uSvKD1d`<TF5kdWjmTY0m4o}n1ZrQD59o68On?!9amLzhrSb(3BYr3Qqx0>*{B};uBz-d|=P=~v5Lck^rgc~ZV>F$s`@#is?cIrNKI?ylM0!j4Uhsm4b%O$TD!<6pPC-D$$r|a+ZE79DILOXV&<J>_jnOCSH1;z{Q8V+;YW-mu^6Iw3gE_cEh?_T2?qK51qi?S9H(`@q4J;XK}=UlLO+14U4*B{9rxyQBV8JP_BiTvr<_iI^nXTH0=@sudb#^R}Ld|W!B>yvf&I{P@s#`WxINa&3cjOe36jd^>*F7@fvM;!36OR`ng;pSC$0xSp-a97{n`YW*jgkTL!5(g?XUUuM>)=W9z3BgOV$B>lPr|Bk&0Hvs#9TI(P4i@yK%LP2MEz?c$farGLDG~jJBW%ZB47=QOoT01Q;-mC3I%yH>>=lxy4Lgh%fW?oNv`^5WK7d9Pvb)`{+M0tc@n*X6uh<YEbZtlf$!&52kmAo#tIPp=|D&$&2KT@Um`v!)r8#;mYENdfUlA|FQdKvtYxa8>n>==3)q<cEuSO(&HOB#Zz@Tp`YtToVGPQ;Gy1l)6kGE>8H9OfoJg@j&y*ewg^dLK)p{N1yIxy0cukp~B4ox>??lsR1)q3WMX%du7bDjUR7{spaD!?B5-F41ve>Ww8)x`1k=-l-z!n;RyvL)OJA@Wf;wdWuL<R>?NUJM!8GY#rqM{6qDdwLk13kn|m0!@APJvy#qWV`lpl;pgVVOdV9^(&r$JPTG7DmRa40)hZn@}b>xqxLD<P&67ywMRyFjHzn00;k@zSiR|&`7+{iq$fw_gYA9~fIwyR5e1SoD(Dm(+R@ot^o8xU@B%kCb+9Vt^!dyQSZMlu<w1$$#PbSppt`FNm1#e(!?WG^vY*N9_A>!YKRrzYn$y1}fy8OXHE%5Begx=o$A0d4yc>{0TJ)fvEs*`O?r|+qXDk<&;_se&@SE!tB{XqX|NPS|pK$s!1hDHI7PV8KFyRxm@^R&S!pl!w;{MS;=wG&E7s>8fivDd%=QmSw&9DbBL*`q~4UlV)d55Q*m|J1@b+&>IlPHJz6Oh9!74lndM@}yu?v5WyufZNa0|^hba@OKVox})0_lj8ec%^zX^ha#QVa5<}#*auo=%vE~speI4qU(5hssPj@g21H1e76|oa$ILz4wVa=g?X}gA5Z4+<O_mF-RNOGizk@mD<@FpMvOWUt?nSyTs?@L<>TsmxbezO@o?UeV}FY8Ve$1kgcvaf+^d8{#YL!(s4P>M{P2yzJ*{f(z|Hfk&?CYB;dz{+nglw~D<wUPi*qD&I1E4!QoKhT)+5p4;ntE>x1`^Wc&*^pmFh#Qd*tNMp+CCZaxV08u8#0l{|;0!9XWb(<R~OKhn-j=!D=dTog^nIScNHe=5p}`9=<$k`bMBbcZNM4bevbf^ly=}#W)Ct!gzQll;Y?CnHEyq?~n~!e|%Jn;3io22d;gU*6*G$^PFj)no(%%cb(T!14vE-6K~}1I+${a4OoF3QHPkPIJawy2~RUVHPoZokVD|x?wx7u4VoM)`72h^$w5_J#*_TNw|lag=RRkG&OO^fCkh)-bDX<c<YaS_Cn}1|@zR6Ok#oQ!%cmX<67jF{<NBY0_waDJK~B{5dr;=^RIy|4>l{4f?W4_~d(Usqk~i==iuG3D-`P-qbD~(*yk)kxp2UzON{3qY$Uh>y#~d1z1<v8y;P&m&hXul7x2+PMzfE=9z4EwPE05@P1H^iZ*Zy`-a<=GpTofy81ucFzG~e|>C|1?4=ST4ENR#D$=b&T`f3wr#xm2kq&~x<ZjeAY?pMOBVKO9j2(0A%v!F0&S{(2@yS&7#Dm`a8dAD7L?E#gl!;U^mqAN^yE$>-WvggCX))E{H3MG~d)8SnhAa2Or_<4XICp07r}v*-0uNq%7dC%ac4b>!zYUT95f%1>0}M^_R^1pZOq0k-dM!p8veqn&k?eB3hoNPJ`CW>r4dw$~?{RuJnf-7xl}_W39)pMPf*K9$_*$G8T}iut%-`BACG-XysREV*QcKgm&k)Hk1I*7W1Xm}80}`{d-tCkFhhO8d&M?-;xHt*1X<#gY2U!HxlB`!%VxzqQ`}2Xo7RNB'


class SafeParser(argparse.ArgumentParser):
    def error(self, message):
        # Never echo accidentally supplied phrase words in an argparse error.
        self.exit(2, 'Invalid options. Use --help; enter words only at the private prompt.\n')


def normalize_phrase(value):
    normalized = ' '.join(unicodedata.normalize('NFKD', value).split())
    words = normalized.split(' ')
    if len(words) != 12:
        raise ValueError('Enter exactly 12 English BIP39 words.')
    data = zlib.decompress(base64.b85decode(WORDLIST_B85))
    if hashlib.sha256(data).hexdigest() != WORDLIST_SHA256:
        raise ValueError('Bundled wordlist integrity check failed.')
    indexes = {word: i for i, word in enumerate(data.decode('ascii').splitlines())}
    if len(indexes) != 2048 or any(word not in indexes for word in words):
        raise ValueError('Phrase contains a word outside the English BIP39 list.')
    bits = 0
    for word in words:
        bits = (bits << 11) | indexes[word]
    entropy = (bits >> 4).to_bytes(16, 'big')
    if bits & 15 != hashlib.sha256(entropy).digest()[0] >> 4:
        raise ValueError('The 12-word phrase has an invalid checksum; recheck your backup.')
    return normalized


def derive_seed(phrase, passphrase=''):
    phrase = normalize_phrase(phrase)
    salt = ('mnemonic' + unicodedata.normalize('NFKD', passphrase)).encode('utf-8')
    return hashlib.pbkdf2_hmac('sha512', phrase.encode('utf-8'), salt, 2048, dklen=64)


def private_parent(output):
    """Walk by directory descriptors: no symlinks or unsafe writable ancestors."""
    path = Path(os.path.abspath(os.path.expanduser(str(output))))
    if path.name in ('', '.', '..'):
        raise ValueError('Choose a seed filename inside a private directory.')
    uid = os.geteuid()
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in path.parent.parts[1:]:
            try:
                os.mkdir(part, 0o700, dir_fd=fd)
            except FileExistsError:
                pass
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = child
            info = os.fstat(fd)
            sticky_root = info.st_uid == 0 and bool(info.st_mode & stat.S_ISVTX)
            if info.st_uid not in (0, uid) or (info.st_mode & 0o022 and not sticky_root):
                raise ValueError('Output has an unsafe writable directory ancestor.')
        info = os.fstat(fd)
        if info.st_uid != uid or info.st_mode & 0o077:
            raise ValueError('Output directory must be owned by you and private (mode 0700).')
        return path, fd
    except BaseException:
        os.close(fd)
        raise


def write_seed(output, seed):
    if len(seed) != 64:
        raise ValueError('Wallet seed must contain exactly 64 bytes.')
    path, parent = private_parent(output)
    temporary = '.cashu-seed-' + secrets.token_hex(16)
    created = False
    try:
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                     0o600, dir_fd=parent)
        created = True
        with os.fdopen(fd, 'wb') as stream:
            stream.write(seed)
            stream.flush()
            os.fsync(stream.fileno())
        # Atomic publication with no replacement, including existing symlinks.
        os.link(temporary, path.name, src_dir_fd=parent, dst_dir_fd=parent,
                follow_symlinks=False)
        os.unlink(temporary, dir_fd=parent)
        created = False
        os.fsync(parent)
        return path
    finally:
        if created:
            os.unlink(temporary, dir_fd=parent)
        os.close(parent)


def main():
    parser = SafeParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path.home() / '.local/share/lightning-goats-cashu/seed-staging/wallet-seed.bin',
                        help='private 64-byte output; existing files are never replaced')
    parser.add_argument('--passphrase', action='store_true',
                        help='also prompt for an existing additional BIP39 passphrase (not a wallet PIN)')
    args = parser.parse_args()
    if not sys.stdin.isatty() or not sys.stderr.isatty():
        parser.exit(2, 'Run interactively in a terminal; redirected secret input is refused.\n')
    if os.geteuid() == 0:
        parser.exit(2, 'Run as a non-root operator or the isolated wallet user.\n')
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    warnings.simplefilter('error', getpass.GetPassWarning)
    os.umask(0o077)
    try:
        path, parent = private_parent(args.output)
        try:
            try:
                os.stat(path.name, dir_fd=parent, follow_symlinks=False)
            except FileNotFoundError:
                pass
            else:
                raise FileExistsError
        finally:
            os.close(parent)
        print('Dedicated Cashu wallet seed preparation. Input is hidden; nothing is sent over the network.')
        phrase = normalize_phrase(getpass.getpass('12 seed words: '))
        repeated = normalize_phrase(getpass.getpass('Enter the same 12 words again: '))
        if not hmac.compare_digest(phrase, repeated):
            raise ValueError('The two entries do not match.')
        passphrase = ''
        if args.passphrase:
            passphrase = getpass.getpass('Additional BIP39 passphrase: ')
            if not hmac.compare_digest(passphrase.encode(), getpass.getpass('Repeat passphrase: ').encode()):
                raise ValueError('The two passphrase entries do not match.')
        saved = write_seed(path, derive_seed(phrase, passphrase))
        print(f'Saved private 64-byte seed file: {saved}')
        print('No wallet service was initialized or started. Keep your original offline backup.')
        return 0
    except FileExistsError:
        print('Refused: a seed file or link already exists. Nothing was replaced.', file=sys.stderr)
    except ValueError as error:
        print(str(error), file=sys.stderr)
    except (EOFError, KeyboardInterrupt):
        print('\nCanceled; no phrase was logged.', file=sys.stderr)
    except getpass.GetPassWarning:
        print('Could not hide terminal input; seed entry was refused.', file=sys.stderr)
    except OSError:
        print('Could not create the private seed file. Check destination ownership and permissions.', file=sys.stderr)
    return 1


if __name__ == '__main__':
    raise SystemExit(main())
